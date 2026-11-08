use super::*;
use serde_json::json;
use std::io::Write;

fn manifest(name: &str, dependencies: serde_json::Value) -> Vec<u8> {
    json!({"name":name,"version":"1.0.0","fhirVersions":["4.3.0"],"dependencies":dependencies})
        .to_string()
        .into_bytes()
}

fn spec(path: &Path) -> PackageSpec {
    PackageSpec::Table {
        id: None,
        version: None,
        path: Some(path.display().to_string()),
        url: None,
    }
}

fn archive(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let encoder = flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::fast(),
    );
    let mut tar = tar::Builder::new(encoder);
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, &data[..]).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
}

#[test]
fn directory_and_archive_have_the_same_identity_and_are_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = manifest("private.test", json!({}));
    std::fs::write(dir.path().join("package.json"), &bytes).unwrap();
    let sources = PreparedSources::from_config(&[spec(dir.path())], FhirVersion::R4B).unwrap();
    assert_eq!(sources.local_packages()[0].name, "private.test");
    assert!(PreparedSources::from_config(&[spec(dir.path())], FhirVersion::R4).is_err());
    let tgz = dir.path().join("package.tgz");
    archive(&tgz, &[("package/package.json", bytes.clone())]);
    let archive_before = std::fs::read(&tgz).unwrap();
    let sources = PreparedSources::from_config(&[spec(&tgz)], FhirVersion::R4B).unwrap();
    let root = sources.local_packages()[0].path.clone();
    assert_eq!(std::fs::read(root.join("package.json")).unwrap(), bytes);
    assert_eq!(std::fs::read(tgz).unwrap(), archive_before);
    drop(sources);
    assert!(
        !root.exists(),
        "owned extraction must be removed after the manager drops"
    );
}

#[test]
fn actual_ru_core_archive_is_a_generic_source() {
    let dir = tempfile::tempdir().unwrap();
    let tgz = dir.path().join("ru-core.tgz");
    std::fs::write(
        &tgz,
        include_bytes!("../../tests/fixtures/ru-core/package.tgz"),
    )
    .unwrap();
    let sources = PreparedSources::from_config(&[spec(&tgz)], FhirVersion::R4B).unwrap();
    let package = &sources.local_packages()[0];
    assert_eq!((&*package.name, &*package.version), ("ru.core", "1"));
    assert!(
        package
            .path
            .join("StructureDefinition-core-patient.json")
            .is_file()
    );
    assert_eq!(
        sources.packages[&("ru.core".into(), "1".into())]
            .1
            .dependencies
            .len(),
        3
    );
}

#[test]
fn offline_closure_uses_cached_transitive_archives_and_reports_the_exact_missing_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root.tgz");
    archive(
        &root,
        &[(
            "package/package.json",
            manifest("private.root", json!({"private.dep":"1.0.0"})),
        )],
    );
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    let roots = [LoadedPackage {
        id: "private.root".into(),
        version: Some("1.0.0".into()),
        path: Some(root.display().to_string()),
        url: None,
    }];
    let mut sources = PreparedSources::from_config(&[spec(&root)], FhirVersion::R4B).unwrap();
    let error = sources
        .prepare_offline(&roots, &HashSet::new(), &cache, FhirVersion::R4B)
        .unwrap_err();
    assert!(
        error.contains("private.dep@1.0.0 (required by private.root@1.0.0)"),
        "{error}"
    );
    archive(
        &cache.join("private.dep-1.0.0.tgz"),
        &[(
            "package/package.json",
            manifest("private.dep", json!({"private.leaf":"1.0.0"})),
        )],
    );
    let installed = HashSet::from(["private.leaf@1.0.0".into()]);
    sources
        .prepare_offline(&roots, &installed, &cache, FhirVersion::R4B)
        .unwrap();
    assert_eq!(sources.local_packages().len(), 2);
    // Cycles terminate too; the manager's graph traversal handles their ordering.
    sources
        .packages
        .get_mut(&("private.dep".into(), "1.0.0".into()))
        .unwrap()
        .1
        .dependencies = std::collections::HashMap::from([("private.root".into(), "1.0.0".into())]);
    sources
        .prepare_offline(&roots, &HashSet::new(), &cache, FhirVersion::R4B)
        .unwrap();
}

#[tokio::test]
async fn offline_registry_blocks_all_remote_operations_and_keeps_archive_alive() {
    use octofhir_canonical_core::{config::PackageSpec, traits::AsyncRegistry};
    let dir = tempfile::tempdir().unwrap();
    let tgz = dir.path().join("package.tgz");
    archive(
        &tgz,
        &[("package/package.json", manifest("private.test", json!({})))],
    );
    let sources = PreparedSources::from_config(&[spec(&tgz)], FhirVersion::R4B).unwrap();
    let root = sources.local_packages()[0].path.clone();
    let registry = SourceRegistry::new(None, sources);
    assert!(root.exists());
    let error = registry
        .get_package_metadata("private.missing", "2.0.0")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("packages.offline") && error.contains("private.missing@2.0.0"));
    assert!(
        registry
            .download_package(&PackageSpec {
                name: "x".into(),
                version: "1".into(),
                priority: 1,
                url: Some("http://127.0.0.1:1/never".into())
            })
            .await
            .is_err()
    );
    assert!(registry.list_versions("x").await.is_err());
    assert!(registry.search_packages("x").await.is_err());
    drop(registry);
    assert!(!root.exists());
}

#[test]
fn unsafe_indexes_missing_files_and_identities_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        manifest("private.test", json!({})),
    )
    .unwrap();
    for files in [
        json!([{"filename":"../outside.json"}]),
        json!({"../outside.json":{}}),
        json!({"missing.json":{}}),
        json!({"test.json":{"resourceType":"../../outside"}}),
    ] {
        std::fs::write(
            dir.path().join(".index.json"),
            json!({"files":files}).to_string(),
        )
        .unwrap();
        assert!(PreparedSources::from_config(&[spec(dir.path())], FhirVersion::R4B).is_err());
    }
    for name in ["../x", "C:\\x", "@scope/name", "", ".", ".."] {
        assert!(validate_identity(name, "1").is_err());
    }
    assert!(validate_identity("private.test", "^1.0.0").is_err());
}

#[test]
fn unsafe_archives_duplicates_oversized_entries_and_corrupt_gzip_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsafe.tgz");
    for entry_type in [
        tar::EntryType::Symlink,
        tar::EntryType::Link,
        tar::EntryType::Fifo,
        tar::EntryType::GNUSparse,
    ] {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_size(0);
        header.set_mode(0o644);
        header.set_link_name("outside").unwrap();
        header.set_cksum();
        tar.append_data(&mut header, "package/link", &[][..])
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(archive::extract(&path).is_err());
    }
    archive(
        &path,
        &[
            ("package/package.json", manifest("private.test", json!({}))),
            ("package/package.json", manifest("private.other", json!({}))),
        ],
    );
    assert!(archive::extract(&path).unwrap_err().contains("duplicate"));
    // Write the raw header so tar's safe builder cannot sanitize the hostile path.
    for filename in [
        "../outside",
        "/absolute",
        "package/../../outside",
        "C:\\escape",
    ] {
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..filename.len()].copy_from_slice(filename.as_bytes());
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        let mut encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        encoder.write_all(header.as_bytes()).unwrap();
        encoder.write_all(&[0; 1024]).unwrap();
        encoder.finish().unwrap();
        assert!(archive::extract(&path).is_err(), "{filename}");
    }
    let mut header = tar::Header::new_gnu();
    header.set_path("package/huge.json").unwrap();
    header.set_size(MAX_FILE_BYTES + 1);
    header.set_mode(0o644);
    header.set_cksum();
    let mut encoder = flate2::write::GzEncoder::new(
        std::fs::File::create(&path).unwrap(),
        flate2::Compression::fast(),
    );
    encoder.write_all(header.as_bytes()).unwrap();
    encoder.finish().unwrap();
    assert!(archive::extract(&path).unwrap_err().contains("size limits"));
    archive(
        &path,
        &[("package/package.json", manifest("private.test", json!({})))],
    );
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 8;
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    assert!(archive::extract(&path).is_err());
}

#[test]
fn explicit_identity_and_cached_root_release_must_match() {
    let dir = tempfile::tempdir().unwrap();
    let tgz = dir.path().join("private.test-1.0.0.tgz");
    archive(
        &tgz,
        &[("package/package.json", manifest("private.test", json!({})))],
    );
    for (id, version) in [(Some("other"), None), (None, Some("2.0.0"))] {
        let spec = PackageSpec::Table {
            id: id.map(str::to_owned),
            version: version.map(str::to_owned),
            path: Some(tgz.display().to_string()),
            url: None,
        };
        assert!(
            PreparedSources::from_config(&[spec], FhirVersion::R4B)
                .err()
                .unwrap()
                .contains("identity")
        );
    }
    let roots = [LoadedPackage {
        id: "private.test".into(),
        version: Some("1.0.0".into()),
        path: None,
        url: None,
    }];
    let mut sources = PreparedSources::default();
    let error = sources
        .prepare_offline(&roots, &HashSet::new(), dir.path(), FhirVersion::R4)
        .unwrap_err();
    assert!(error.contains("does not declare FHIR R4"), "{error}");
}

#[test]
fn published_legacy_fhir_version_field_is_normalized_without_changing_input() {
    let dir = tempfile::tempdir().unwrap();
    let raw =
        json!({"name":"private.older","version":"1.0.0","fhir-version-list":["4.3.0"]}).to_string();
    let original = dir.path().join("package.json");
    std::fs::write(&original, &raw).unwrap();
    std::fs::write(
        dir.path().join("ValueSet-test.json"),
        json!({"resourceType":"ValueSet","id":"test"}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o444)).unwrap();
    }
    let sources = PreparedSources::from_config(&[spec(dir.path())], FhirVersion::R4B).unwrap();
    let copy = &sources.local_packages()[0].path;
    assert_ne!(copy, dir.path());
    assert!(copy.join("ValueSet-test.json").is_file());
    let manifest: PackageManifest =
        serde_json::from_slice(&std::fs::read(copy.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest.fhir_versions.unwrap(), ["4.3.0"]);
    assert_eq!(std::fs::read_to_string(&original).unwrap(), raw);
    let archive_path = dir.path().join("older.tgz");
    archive(&archive_path, &[("package/package.json", raw.into_bytes())]);
    let sources = PreparedSources::from_config(&[spec(&archive_path)], FhirVersion::R4B).unwrap();
    let manifest: PackageManifest = serde_json::from_slice(
        &std::fs::read(sources.local_packages()[0].path.join("package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.fhir_versions.unwrap(), ["4.3.0"]);
}
