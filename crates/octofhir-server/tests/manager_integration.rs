use octofhir_server::{AppConfig, canonical};

#[tokio::test]
async fn manager_loads_packages_when_online_opt_in() {
    // Skip unless explicitly opted-in to avoid network in CI
    if std::env::var("OCTOFHIR_TEST_CANONICAL_ONLINE")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!(
            "skipping canonical manager online test (set OCTOFHIR_TEST_CANONICAL_ONLINE=1 to run)"
        );
        return;
    }

    // Encourage quick init to reduce cost
    unsafe {
        std::env::set_var("FHIRPATH_QUICK_INIT", "1");
    }

    // Use default FCM storage dirs or allow overriding via env
    let cfg = AppConfig {
        packages: octofhir_server::config::PackagesConfig {
            // Choose widely available packages; requires network access
            load: vec![octofhir_server::config::PackageSpec::Simple(
                "hl7.fhir.r4b.core#4.3.0".into(),
            )],
            path: None,
        },
        ..AppConfig::default()
    };

    // Build registry using real manager
    let reg = canonical::init_from_config_async(&cfg)
        .await
        .expect("canonical init");
    let guard = reg.read().unwrap();
    let pkgs = guard.list();
    assert!(pkgs.iter().any(|p| p.id == "hl7.fhir.r4b.core"));
}
