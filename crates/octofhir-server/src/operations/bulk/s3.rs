use std::path::Path;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sigv4::http_request::SignableBody;
use reqwest::{Client, Method, Response, StatusCode, header};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::config::BulkExportS3Config;
use crate::server::AppState;
use crate::storage_providers::{StorageProvider, signing, store};

use super::NDJSON_CONTENT_TYPE;

pub(crate) fn job_destination(body: Option<&Value>) -> Result<Option<BulkExportS3Config>, String> {
    body.and_then(|body| body.get("config"))
        .and_then(|config| config.get("s3"))
        .filter(|s3| !s3.is_null())
        .map(|s3| {
            serde_json::from_value(s3.clone()).map_err(|_| "Invalid export S3 destination".into())
        })
        .transpose()
}

pub(crate) async fn check_destination(
    state: &AppState,
    destination: &BulkExportS3Config,
) -> Result<(), String> {
    let resolved = store::resolve(&state.db_pool, &destination.provider, &state.egress)
        .await
        .map_err(|_| "S3 export destination is unavailable")?;
    check_bucket(&resolved.provider, &destination.bucket)
}

fn check_bucket(provider: &StorageProvider, bucket: &str) -> Result<(), String> {
    if provider
        .export_buckets
        .iter()
        .any(|allowed| allowed == bucket)
    {
        Ok(())
    } else {
        Err("Bucket is not allowed for this S3 export provider".into())
    }
}

pub(crate) struct ExportBucket<'a> {
    state: &'a AppState,
    provider: StorageProvider,
    credentials: Credentials,
    bucket: String,
    client: Client,
}

impl<'a> ExportBucket<'a> {
    pub(crate) async fn open(
        state: &'a AppState,
        destination: BulkExportS3Config,
    ) -> Result<Self, String> {
        Self::connect(state, destination, false).await
    }

    pub(crate) async fn open_for_upload(
        state: &'a AppState,
        destination: BulkExportS3Config,
    ) -> Result<Self, String> {
        Self::connect(state, destination, true).await
    }

    async fn connect(
        state: &'a AppState,
        destination: BulkExportS3Config,
        upload: bool,
    ) -> Result<Self, String> {
        destination.validate()?;
        let resolved = store::resolve(&state.db_pool, &destination.provider, &state.egress).await?;
        if upload {
            check_bucket(&resolved.provider, &destination.bucket)?;
        }
        let credentials = resolved.credentials().await?;
        let provider = resolved.provider;
        let client = state
            .egress
            .client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| "Cannot create bulk export S3 client")?;
        Ok(Self {
            state,
            provider,
            credentials,
            bucket: destination.bucket,
            client,
        })
    }

    fn job_prefix(job_id: Uuid) -> String {
        format!("bulk-export/{job_id}/")
    }

    fn object_url(&self, job_id: Uuid, filename: &str) -> Result<url::Url, String> {
        let key = format!("{}{filename}", Self::job_prefix(job_id));
        self.provider.object_url(&self.bucket, &key, None)
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
        payload: SignableBody<'_>,
    ) -> Result<Response, String> {
        let mut request = builder
            .build()
            .map_err(|_| "Cannot build bulk export S3 request")?;
        self.state.egress.check_url(request.url())?;
        signing::sign(
            &mut request,
            &self.credentials,
            &self.provider.region,
            payload,
        )?;
        self.client
            .execute(request)
            .await
            .map_err(|_| "Bulk export S3 request failed".into())
    }

    pub(crate) async fn put(
        &self,
        job_id: Uuid,
        filename: &str,
        path: &Path,
        size: u64,
    ) -> Result<(), String> {
        let mut file = File::open(path)
            .await
            .map_err(|_| "Cannot open staged export file")?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .await
                .map_err(|_| "Cannot hash staged export file")?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        file.rewind()
            .await
            .map_err(|_| "Cannot rewind staged export file")?;
        let upload = self
            .client
            .put(self.object_url(job_id, filename)?)
            .header(header::CONTENT_LENGTH, size)
            .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
            .timeout(Duration::from_secs(900))
            .body(file);
        let response = self
            .send(
                upload,
                SignableBody::Precomputed(hex::encode(hash.finalize())),
            )
            .await?;
        expect_status(&response, "PUT", StatusCode::OK)
    }

    pub(crate) async fn get(
        &self,
        job_id: Uuid,
        filename: &str,
        method: Method,
    ) -> Result<Option<Response>, String> {
        let request = self
            .client
            .request(method, self.object_url(job_id, filename)?);
        let response = self.send(request, SignableBody::Bytes(&[])).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        expect_status(&response, "read", StatusCode::OK)?;
        Ok(Some(response))
    }

    pub(crate) async fn delete_job(&self, job_id: Uuid) -> Result<(), String> {
        let prefix = Self::job_prefix(job_id);
        let mut bucket = self.provider.object_url(&self.bucket, "", None)?;
        bucket
            .path_segments_mut()
            .map_err(|_| "Invalid export bucket URL")?
            .pop_if_empty();
        bucket
            .query_pairs_mut()
            .append_pair("list-type", "2")
            .append_pair("prefix", &prefix);

        let mut continuation = None::<String>;
        loop {
            let mut list = bucket.clone();
            if let Some(token) = &continuation {
                list.query_pairs_mut()
                    .append_pair("continuation-token", token);
            }
            let response = self
                .send(self.client.get(list), SignableBody::Bytes(&[]))
                .await?;
            expect_status(&response, "list", StatusCode::OK)?;
            let listing: ObjectListing = quick_xml::de::from_str(
                &response
                    .text()
                    .await
                    .map_err(|_| "Cannot read bulk export S3 listing")?,
            )
            .map_err(|_| "Invalid bulk export S3 listing")?;
            for object in listing.contents {
                let filename = object
                    .key
                    .strip_prefix(&prefix)
                    .filter(|name| {
                        !name.is_empty()
                            && !name.contains(['/', '\\'])
                            && !matches!(*name, "." | "..")
                    })
                    .ok_or("S3 listing returned an object outside the export job")?;
                let request = self.client.delete(self.object_url(job_id, filename)?);
                let response = self.send(request, SignableBody::Bytes(&[])).await?;
                expect_status(&response, "DELETE", StatusCode::NO_CONTENT)?;
            }
            if !listing.is_truncated {
                return Ok(());
            }
            let token = listing
                .next_continuation_token
                .filter(|token| Some(token) != continuation.as_ref())
                .ok_or("S3 listing did not return another continuation token")?;
            continuation = Some(token);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ObjectListing {
    #[serde(default)]
    contents: Vec<ListedObject>,
    is_truncated: bool,
    next_continuation_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListedObject {
    key: String,
}

fn expect_status(response: &Response, action: &str, expected: StatusCode) -> Result<(), String> {
    if response.status() == expected {
        Ok(())
    } else {
        Err(format!(
            "Bulk export S3 {action} returned HTTP {}",
            response.status().as_u16()
        ))
    }
}
