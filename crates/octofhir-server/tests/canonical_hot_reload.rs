use octofhir_server::{AppConfig, canonical};

#[tokio::test]
async fn rebuild_from_config_swaps_registry_contents() {
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
    // Initial config with one package
    let cfg1 = AppConfig {
        packages: octofhir_server::config::PackagesConfig {
            load: vec![octofhir_server::config::PackageSpec::Simple(
                "hl7.fhir.r4b.core#4.3.0".into(),
            )],
            path: None,
        },
        ..AppConfig::default()
    };

    let reg = canonical::init_from_config_async(&cfg1)
        .await
        .expect("canonical init");
    canonical::set_registry(reg);
    assert!(canonical::with_registry(|r| r.list().len()).unwrap_or_default() >= 1);

    // New config with different packages
    let cfg2 = AppConfig {
        packages: octofhir_server::config::PackagesConfig {
            load: vec![
                octofhir_server::config::PackageSpec::Simple("hl7.terminology#5.5.0".into()),
                octofhir_server::config::PackageSpec::Simple("custom.package".into()),
            ],
            path: None,
        },
        ..AppConfig::default()
    };

    let _ = canonical::rebuild_from_config_async(&cfg2).await;
    let list = canonical::with_registry(|r| r.list().to_vec()).unwrap();
    assert!(list.iter().any(|p| p.id == "hl7.terminology"));
}
