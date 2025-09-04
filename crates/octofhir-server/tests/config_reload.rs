use std::{
    fs,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use octofhir_server::config::loader;

// Unused helper removed

#[test]
fn file_watching_triggers_reload_and_updates_shared_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("octofhir.toml");
    // Create a Tokio runtime for watcher to spawn tasks on
    let rt = tokio::runtime::Runtime::new().unwrap();

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

    let _guard = octofhir_server::config_watch::start_config_watcher(
        path.clone(),
        shared_cfg.clone(),
        rt.handle().clone(),
    );

    // Give watcher a brief moment to start
    thread::sleep(Duration::from_millis(300));

    // Modify the file to change logging level and search.default_count
    let updated = base
        .replace("level = \"info\"", "level = \"debug\"")
        .replace("default_count = 5", "default_count = 7");
    fs::write(&path, &updated).unwrap();

    // Poll for up to 10 seconds for the change to be applied
    let mut applied = false;
    for i in 0..100 {
        thread::sleep(Duration::from_millis(100));
        if let Ok(guard) = shared_cfg.read() {
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

#[test]
fn invalid_reload_does_not_replace_shared_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("octofhir.toml");
    // Create a Tokio runtime for watcher to spawn tasks on
    let rt = tokio::runtime::Runtime::new().unwrap();

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

    let _guard = octofhir_server::config_watch::start_config_watcher(
        path.clone(),
        shared_cfg.clone(),
        rt.handle().clone(),
    );

    // Write invalid config (default_count > max_count)
    let invalid = base.replace(
        "default_count = 5\nmax_count = 10",
        "default_count = 50\nmax_count = 10",
    );
    fs::write(&path, invalid).unwrap();

    // Wait for potential reload attempt
    thread::sleep(Duration::from_millis(1000));

    let guard = shared_cfg.read().unwrap();
    assert_eq!(guard.search.default_count, 5);
    assert_eq!(guard.search.max_count, 10);
}
