use std::{env, fs};

use octofhir_server::config::loader::load_config;

#[test]
fn config_parsing_and_env_overrides_and_validation() {
    // Create a temporary TOML configuration file
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("octofhir.toml");

    let toml_content = r#"
[server]
host = "127.0.0.1"
port = 8081
read_timeout_ms = 1000
write_timeout_ms = 1000
body_limit_bytes = 1024

[storage]
backend = "postgres"

[storage.postgres]
host = "localhost"
port = 5432
database = "octofhir"
username = "test"
password = "test"

[search]
default_count = 5
max_count = 10

[logging]
level = "debug"

[otel]
enabled = false

[packages]
load = ["hl7.fhir.r4b.core#4.3.0", "hl7.terminology#5.5.0"]
"#;
    fs::write(&path, toml_content).expect("write toml");

    // 1) Valid config parses
    let cfg = load_config(path.to_str()).expect("should parse config");
    assert_eq!(cfg.server.port, 8081);
    assert_eq!(cfg.search.default_count, 5);
    assert_eq!(cfg.search.max_count, 10);
    assert_eq!(cfg.logging.level.to_ascii_lowercase(), "debug");
    assert_eq!(cfg.packages.load.len(), 2);

    // 2) Env override should win over file
    unsafe {
        env::set_var("OCTOFHIR__SEARCH__DEFAULT_COUNT", "9");
    }
    let cfg_env = load_config(path.to_str()).expect("should parse config with env overrides");
    assert_eq!(cfg_env.search.default_count, 9);
    // cleanup env var
    unsafe {
        env::remove_var("OCTOFHIR__SEARCH__DEFAULT_COUNT");
    }

    // 3) Invalid config (default > max) should error
    let invalid_path = dir.path().join("invalid.toml");
    let invalid_toml = r#"
[storage.postgres]
host = "localhost"
port = 5432
database = "octofhir"
username = "test"
password = "test"

[search]
default_count = 50
max_count = 10
"#;
    fs::write(&invalid_path, invalid_toml).expect("write invalid toml");
    let err = load_config(invalid_path.to_str()).expect_err("expected validation error");
    assert!(err.contains("default_count must be <="));
}
