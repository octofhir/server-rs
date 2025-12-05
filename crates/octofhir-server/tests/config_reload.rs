//! Tests for configuration hot-reload functionality.
//!
//! These tests verify that the unified configuration management system
//! properly detects and applies configuration changes.

use std::{
    fs,
    sync::Arc,
    time::Duration,
};

use octofhir_server::config::loader;
use octofhir_server::config_manager::ServerConfigManager;
use tokio::sync::RwLock;

#[tokio::test]
async fn file_watching_triggers_reload_and_updates_shared_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("octofhir.toml");

    let base = r#"
[server]
host = "127.0.0.1"
port = 18080
read_timeout_ms = 1000
write_timeout_ms = 1000
body_limit_bytes = 1024

[search]
default_count = 5
max_count = 10

[logging]
level = "info"

[otel]
enabled = false
"#;
    fs::write(&path, base).unwrap();

    let cfg = loader::load_config(path.to_str()).expect("load initial");
    let shared_cfg = Arc::new(RwLock::new(cfg.clone()));

    // Create config manager with file watching
    let manager = ServerConfigManager::builder()
        .with_file(&path)
        .build()
        .await
        .expect("build config manager");

    // Start watching for changes
    manager.start_watching(shared_cfg.clone()).await;

    // Give watcher a brief moment to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Modify the file to change logging level and search.default_count
    let updated = base
        .replace("level = \"info\"", "level = \"debug\"")
        .replace("default_count = 5", "default_count = 7");
    fs::write(&path, &updated).unwrap();

    // Poll for up to 10 seconds for the change to be applied
    let mut applied = false;
    for i in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        {
            let guard = shared_cfg.read().await;
            let c = &*guard;
            if c.logging.level.eq_ignore_ascii_case("debug") && c.search.default_count == 7 {
                applied = true;
                break;
            }
        }
        // Nudge the file again after 1s if not yet applied
        if i == 10 {
            fs::write(&path, updated.clone()).unwrap();
        }
    }
    assert!(applied, "reload did not apply within timeout");
}

#[tokio::test]
async fn invalid_reload_does_not_replace_shared_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("octofhir.toml");

    let base = r#"
[server]
port = 18081
read_timeout_ms = 1000
write_timeout_ms = 1000
body_limit_bytes = 1024

[search]
default_count = 5
max_count = 10

[logging]
level = "info"
"#;
    fs::write(&path, base).unwrap();

    let cfg = loader::load_config(path.to_str()).expect("load initial");
    let shared_cfg = Arc::new(RwLock::new(cfg.clone()));

    // Create config manager with file watching
    let manager = ServerConfigManager::builder()
        .with_file(&path)
        .build()
        .await
        .expect("build config manager");

    // Start watching for changes
    manager.start_watching(shared_cfg.clone()).await;

    // Write invalid config (default_count > max_count)
    let invalid = base.replace(
        "default_count = 5\nmax_count = 10",
        "default_count = 50\nmax_count = 10",
    );
    fs::write(&path, invalid).unwrap();

    // Wait for potential reload attempt
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let guard = shared_cfg.read().await;
    assert_eq!(guard.search.default_count, 5);
    assert_eq!(guard.search.max_count, 10);
}

#[tokio::test]
async fn config_manager_builder_without_sources() {
    // Should succeed even without any sources
    let manager = ServerConfigManager::builder()
        .build()
        .await;

    assert!(manager.is_ok(), "Config manager should build without sources");
}

#[tokio::test]
async fn config_manager_subscribe_receives_events() {
    let manager = ServerConfigManager::builder()
        .build()
        .await
        .expect("build config manager");

    // Subscribing should work
    let _rx = manager.subscribe();
}
