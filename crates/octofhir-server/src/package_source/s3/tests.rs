//! Regression coverage for S3 identity, signing, bounds, credentials and caching.
use super::*;
use crate::{config::PackageSpec, package_source::PreparedSources};
use octofhir_core::fhir::FhirVersion;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{any, method},
};

const PACKAGE: &[u8] = include_bytes!("../../../tests/fixtures/ru-core/package.tgz");
#[derive(Clone, serde::Serialize)]
struct TestSource {
    #[serde(flatten)]
    object: S3PackageSource,
    #[serde(skip)]
    connection: crate::storage_providers::StorageProvider,
}
impl std::ops::Deref for TestSource {
    type Target = S3PackageSource;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}
impl std::ops::DerefMut for TestSource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}
impl TestSource {
    fn validate(&self, policy: &EgressPolicy) -> Result<(), String> {
        self.object.validate()?;
        self.connection.validate(policy)?;
        policy.check_url(&self.object.object_url(&self.connection)?)?;
        Ok(())
    }
}
fn source(endpoint: &str) -> TestSource {
    TestSource {
        object: S3PackageSource {
            provider: "test".into(),
            bucket: "private-igs".into(),
            key: "private/ru core+%.tgz".into(),
            sha256: hex::encode(Sha256::digest(PACKAGE)),
            ..Default::default()
        },
        connection: serde_json::from_value(
            serde_json::json!({"id":"test","endpoint":endpoint,"allowHttp":true}),
        )
        .unwrap(),
    }
}
async fn download_with(
    source: &TestSource,
    cache: &Path,
    policy: &Arc<EgressPolicy>,
    resolve: impl FnOnce() -> Result<Credentials, String>,
) -> Result<PathBuf, String> {
    super::download_with(
        &source.object,
        &source.connection,
        cache,
        policy,
        || async { resolve() },
    )
    .await
}
fn fixture_credentials() -> Credentials {
    Credentials::new(
        "fixture-key",
        "fixture-secret",
        Some("fixture-token".into()),
        None,
        "tests",
    )
}
fn policy() -> Arc<EgressPolicy> {
    Arc::new(EgressPolicy::permissive())
}

#[test]
fn source_configuration_rejects_ambiguous_or_unpinned_sources() {
    let good = source("http://127.0.0.1:9000");
    good.validate(&policy()).unwrap();
    let mut bad = good.clone();
    bad.sha256 = String::new();
    assert!(bad.validate(&policy()).is_err());
    for key in [
        "",
        "../package.tgz",
        "a/./b",
        "a//b",
        "a\\b",
        "/package.tgz",
        "a\nb",
    ] {
        let mut bad = good.clone();
        bad.key = key.into();
        assert!(bad.validate(&policy()).is_err(), "{key}");
    }
    for endpoint in [
        "https://id:secret@example.org",
        "https://example.org?signature=secret",
        "https://example.org/base",
        "https://example.org#secret",
        "file:///private",
    ] {
        let mut bad = good.clone();
        bad.connection.endpoint = Some(endpoint.into());
        assert!(bad.validate(&policy()).is_err());
    }
    let mut bad = good.clone();
    bad.connection.allow_http = false;
    assert!(bad.validate(&policy()).is_err());
    assert!(good.validate(&EgressPolicy::default()).is_err());
    let raw = serde_json::json!({"s3":good,"path":"/cannot/select/both"});
    assert!(serde_json::from_value::<PackageSpec>(raw).is_err());
    let raw = serde_json::json!({"s3":good,"url":"https://cannot/select/both"});
    assert!(serde_json::from_value::<PackageSpec>(raw).is_err());
    assert!(serde_json::from_value::<PackageSpec>(serde_json::json!({"s3":good})).is_ok());
}

#[tokio::test]
async fn signed_object_preserves_bytes_version_and_key_and_reuses_only_verified_cache() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-amz-version-id", "release+/1==")
                .insert_header("content-encoding", "gzip")
                .set_body_bytes(PACKAGE),
        )
        .expect(2)
        .mount(&server)
        .await;
    let mut source = source(&server.uri());
    source.version_id = Some("release+/1==".into());
    let cache = tempfile::tempdir().unwrap();
    let path = download_with(&source, cache.path(), &policy(), || {
        Ok(fixture_credentials())
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), PACKAGE);
    let request = server.received_requests().await.unwrap().pop().unwrap();
    assert_eq!(
        urlencoding::decode(request.url.path()).unwrap(),
        "/private-igs/private/ru core+%.tgz"
    );
    assert_eq!(
        request
            .url
            .query_pairs()
            .find(|(k, _)| k == "versionId")
            .unwrap()
            .1,
        "release+/1=="
    );
    assert!(
        request.headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("AWS4-HMAC-SHA256 Credential=fixture-key/")
    );
    assert_eq!(request.headers["x-amz-security-token"], "fixture-token");
    assert!(!request.url.as_str().contains("fixture-token"));
    let sources = PreparedSources::from_config(
        &[PackageSpec::Table {
            id: None,
            version: None,
            path: Some(path.display().to_string()),
            url: None,
        }],
        FhirVersion::R4B,
    )
    .unwrap();
    assert_eq!(sources.local_packages()[0].name, "ru.core");
    std::fs::write(&path, b"corrupt cache").unwrap();
    download_with(&source, cache.path(), &policy(), || {
        Ok(fixture_credentials())
    })
    .await
    .unwrap();
    download_with(&source, cache.path(), &policy(), || {
        Err("credentials must not be read on cache hit".into())
    })
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn failures_do_not_publish_archives_or_expose_response_bodies() {
    for (status, body, version, max, expected) in [
        (
            403,
            b"private-error-and-secret".as_slice(),
            None,
            256 * 1024 * 1024,
            "HTTP 403",
        ),
        (
            200,
            b"wrong bytes".as_slice(),
            None,
            256 * 1024 * 1024,
            "SHA-256 mismatch",
        ),
        (200, PACKAGE, None, 1, "size limit"),
        (
            200,
            PACKAGE,
            Some("wrong-version"),
            256 * 1024 * 1024,
            "version_id",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(status).set_body_bytes(body))
            .mount(&server)
            .await;
        let mut source = source(&server.uri());
        source.version_id = version.map(str::to_owned);
        source.max_archive_bytes = max;
        let cache = tempfile::tempdir().unwrap();
        let error = download_with(&source, cache.path(), &policy(), || {
            Ok(fixture_credentials())
        })
        .await
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("private-error-and-secret"));
        assert_eq!(
            std::fs::read_dir(cache.path().join("sources"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn redirects_and_private_egress_are_blocked_before_credentials_can_escape() {
    let trap = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&trap)
        .await;
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(302).insert_header("Location", trap.uri()))
        .mount(&server)
        .await;
    let source = source(&server.uri());
    let cache = tempfile::tempdir().unwrap();
    let error = download_with(&source, cache.path(), &policy(), || {
        Ok(fixture_credentials())
    })
    .await
    .unwrap_err();
    assert!(error.contains("302"));
    let before = server.received_requests().await.unwrap().len();
    assert!(
        download_with(
            &source,
            cache.path(),
            &Arc::new(EgressPolicy::default()),
            || panic!("must validate egress first")
        )
        .await
        .is_err()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn body_stream_is_bounded_without_content_length() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback(|| async {
        axum::body::Body::from_stream(futures_util::stream::iter([
            Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(b"ab")),
            Ok(axum::body::Bytes::from_static(b"cd")),
        ]))
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut source = source(&endpoint);
    source.max_archive_bytes = 3;
    let cache = tempfile::tempdir().unwrap();
    let error = download_with(&source, cache.path(), &policy(), || {
        Ok(fixture_credentials())
    })
    .await
    .unwrap_err();
    assert!(error.contains("size limit"), "{error}");
    assert_eq!(
        std::fs::read_dir(cache.path().join("sources"))
            .unwrap()
            .count(),
        0
    );
    server.abort();
    let _ = server.await;
}
