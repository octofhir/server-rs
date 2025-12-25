//! Unit tests for X-Skip-Validation header support.
//!
//! These tests verify the header parsing and configuration logic
//! without requiring a full server setup.

use axum::http::HeaderMap;
use octofhir_server::config::ValidationSettings;

/// Helper function from handlers.rs (duplicated for testing)
fn should_skip_validation(headers: &HeaderMap, config: &ValidationSettings) -> bool {
    // Feature must be enabled in config
    if !config.allow_skip_validation {
        return false;
    }

    // Check if X-Skip-Validation header is present and set to "true"
    headers
        .get("X-Skip-Validation")
        .and_then(|h| h.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[test]
fn test_skip_validation_disabled_by_default() {
    let config = ValidationSettings::default();
    assert!(!config.allow_skip_validation);

    let mut headers = HeaderMap::new();
    headers.insert("X-Skip-Validation", "true".parse().unwrap());

    // Should not skip because feature is disabled
    assert!(!should_skip_validation(&headers, &config));
}

#[test]
fn test_skip_validation_header_parsing() {
    let config = ValidationSettings {
        allow_skip_validation: true,
        skip_reference_validation: false,
    };

    // Test with "true"
    let mut headers = HeaderMap::new();
    headers.insert("X-Skip-Validation", "true".parse().unwrap());
    assert!(should_skip_validation(&headers, &config));

    // Test with "TRUE" (case insensitive)
    headers.clear();
    headers.insert("X-Skip-Validation", "TRUE".parse().unwrap());
    assert!(should_skip_validation(&headers, &config));

    // Test with "True"
    headers.clear();
    headers.insert("X-Skip-Validation", "True".parse().unwrap());
    assert!(should_skip_validation(&headers, &config));

    // Test with "false" (should not skip)
    headers.clear();
    headers.insert("X-Skip-Validation", "false".parse().unwrap());
    assert!(!should_skip_validation(&headers, &config));

    // Test with missing header
    headers.clear();
    assert!(!should_skip_validation(&headers, &config));

    // Test with empty header value
    headers.clear();
    headers.insert("X-Skip-Validation", "".parse().unwrap());
    assert!(!should_skip_validation(&headers, &config));
}

#[test]
fn test_validation_config_serde() {
    use serde_json;

    // Test default serialization
    let config = ValidationSettings::default();
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("\"allow_skip_validation\":false"));

    // Test deserialization with explicit value
    let json = r#"{"allow_skip_validation":true}"#;
    let config: ValidationSettings = serde_json::from_str(json).unwrap();
    assert!(config.allow_skip_validation);

    // Test deserialization with default
    let json = r#"{}"#;
    let config: ValidationSettings = serde_json::from_str(json).unwrap();
    assert!(!config.allow_skip_validation); // Should default to false
}

#[test]
fn test_two_layer_security() {
    // Layer 1: Config must enable the feature
    let config_disabled = ValidationSettings {
        allow_skip_validation: false,
        skip_reference_validation: false,
    };

    let mut headers = HeaderMap::new();
    headers.insert("X-Skip-Validation", "true".parse().unwrap());

    // Config disabled, header present -> should not skip
    assert!(!should_skip_validation(&headers, &config_disabled));

    // Layer 2: Header must be present
    let config_enabled = ValidationSettings {
        allow_skip_validation: true,
        skip_reference_validation: false,
    };
    headers.clear();

    // Config enabled, no header -> should not skip
    assert!(!should_skip_validation(&headers, &config_enabled));

    // Both layers satisfied -> should skip
    headers.insert("X-Skip-Validation", "true".parse().unwrap());
    assert!(should_skip_validation(&headers, &config_enabled));
}
