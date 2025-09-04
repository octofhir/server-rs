use octofhir_server::{AppConfig, canonical};

#[tokio::test]
async fn registry_initializes_from_packages_config() {
    // Skip network-dependent test unless opted-in
    if std::env::var("OCTOFHIR_TEST_CANONICAL_ONLINE")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!("skipping canonical manager test (set OCTOFHIR_TEST_CANONICAL_ONLINE=1 to run)");
        return;
    }

    let cfg = AppConfig {
        packages: octofhir_server::config::PackagesConfig {
            load: vec![
                octofhir_server::config::PackageSpec::Simple("hl7.fhir.r4b.core#4.3.0".into()),
                octofhir_server::config::PackageSpec::Simple("hl7.terminology".into()),
                // malformed entries should be ignored gracefully
                octofhir_server::config::PackageSpec::Simple("".into()),
            ],
            path: None,
        },
        ..AppConfig::default()
    };

    let reg = canonical::init_from_config_async(&cfg)
        .await
        .expect("canonical init");
    let guard = reg.read().unwrap();
    let pkgs = guard.list();
    assert!(pkgs.iter().any(|p| p.id == "hl7.fhir.r4b.core"));
}
