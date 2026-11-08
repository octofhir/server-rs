//! Downloaded binaries must provide help/version without reading config or opening a DB.
use std::process::Command;
#[test]
fn information_flags_do_not_initialize_a_server() {
    for (argument, expected) in [
        ("--help", "--config"),
        ("--version", env!("CARGO_PKG_VERSION")),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_octofhir-server"))
            .arg(argument)
            .env("OCTOFHIR_CONFIG", "/does-not-exist/release-smoke.toml")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{argument}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(expected));
        assert!(!stdout.contains("Prometheus metrics initialized"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_octofhir-server"))
        .arg("--unsupported")
        .env("OCTOFHIR_CONFIG", "/does-not-exist/release-smoke.toml")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
}
