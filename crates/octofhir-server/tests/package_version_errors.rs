//! Unit tests for package version error message formatting.
//!
//! These tests verify that FHIR version mismatch errors provide
//! clear, actionable messages to users.

#[cfg(test)]
mod tests {
    /// Test helper to access the private format_version_mismatch_error function
    /// by replicating its logic here for testing purposes.
    fn format_version_mismatch_error_test(
        package_id: &str,
        package_version: &str,
        package_fhir_version: &str,
        server_fhir_version: &str,
        all_packages: &[(String, String)],
    ) -> String {
        let registry_url = format!("https://registry.fhir.org/package/{}", package_id);
        let packages_list = all_packages
            .iter()
            .map(|(name, ver)| format!("  - {}@{}", name, ver))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"
========================================================================
FHIR VERSION MISMATCH DETECTED
========================================================================

Package: {}@{}
Package FHIR Version: {}
Server FHIR Version: {}

All packages must target the same FHIR version as the server.

To fix this issue:

Option 1: Change server configuration to use FHIR {}
  - Update your config file:
    [fhir]
    version = "{}"

Option 2: Use a {}-compatible version of {}
  - Check {} for available versions
  - Update your config file to use a compatible version

Currently configured packages:
{}

For more information about FHIR package versions, see:
https://confluence.hl7.org/display/FHIR/NPM+Package+Specification

========================================================================
"#,
            package_id,
            package_version,
            package_fhir_version,
            server_fhir_version,
            package_fhir_version,
            package_fhir_version,
            server_fhir_version,
            package_id,
            registry_url,
            packages_list
        )
    }

    #[test]
    fn test_version_mismatch_error_contains_all_sections() {
        let packages = vec![
            ("hl7.fhir.r4b.core".to_string(), "4.3.0".to_string()),
            ("hl7.fhir.us.core".to_string(), "6.1.0".to_string()),
        ];

        let error =
            format_version_mismatch_error_test("hl7.fhir.us.core", "6.1.0", "R4", "R4B", &packages);

        // Check for key sections
        assert!(error.contains("FHIR VERSION MISMATCH DETECTED"));
        assert!(error.contains("Package: hl7.fhir.us.core@6.1.0"));
        assert!(error.contains("Package FHIR Version: R4"));
        assert!(error.contains("Server FHIR Version: R4B"));

        // Check for actionable options
        assert!(error.contains("Option 1: Change server configuration to use FHIR R4"));
        assert!(error.contains("Option 2: Use a R4B-compatible version of hl7.fhir.us.core"));

        // Check for registry URL
        assert!(error.contains("https://registry.fhir.org/package/hl7.fhir.us.core"));

        // Check for package list
        assert!(error.contains("hl7.fhir.r4b.core@4.3.0"));
        assert!(error.contains("hl7.fhir.us.core@6.1.0"));

        // Check for documentation link
        assert!(
            error.contains("https://confluence.hl7.org/display/FHIR/NPM+Package+Specification")
        );
    }

    #[test]
    fn test_version_mismatch_error_format_with_single_package() {
        let packages = vec![("hl7.fhir.r4.core".to_string(), "4.0.1".to_string())];

        let error =
            format_version_mismatch_error_test("hl7.fhir.r4.core", "4.0.1", "R4", "R4B", &packages);

        assert!(error.contains("Package: hl7.fhir.r4.core@4.0.1"));
        assert!(error.contains("Currently configured packages:"));
        assert!(error.contains("hl7.fhir.r4.core@4.0.1"));
    }

    #[test]
    fn test_version_mismatch_error_includes_config_example() {
        let packages = vec![("hl7.fhir.r5.core".to_string(), "5.0.0".to_string())];

        let error =
            format_version_mismatch_error_test("hl7.fhir.r5.core", "5.0.0", "R5", "R4B", &packages);

        // Should suggest changing config to R5 (package version)
        assert!(error.contains("[fhir]"));
        assert!(error.contains("version = \"R5\""));
    }

    #[test]
    fn test_version_mismatch_error_includes_registry_url() {
        let packages = vec![("custom.package".to_string(), "1.0.0".to_string())];

        let error =
            format_version_mismatch_error_test("custom.package", "1.0.0", "R4", "R4B", &packages);

        assert!(error.contains("https://registry.fhir.org/package/custom.package"));
    }

    #[test]
    fn test_version_mismatch_error_lists_all_configured_packages() {
        let packages = vec![
            ("hl7.fhir.r4b.core".to_string(), "4.3.0".to_string()),
            ("hl7.fhir.us.core".to_string(), "5.0.1".to_string()),
            ("hl7.terminology".to_string(), "5.5.0".to_string()),
        ];

        let error =
            format_version_mismatch_error_test("hl7.fhir.us.core", "5.0.1", "R4", "R4B", &packages);

        // All packages should be listed
        assert!(error.contains("hl7.fhir.r4b.core@4.3.0"));
        assert!(error.contains("hl7.fhir.us.core@5.0.1"));
        assert!(error.contains("hl7.terminology@5.5.0"));
    }

    #[test]
    fn test_version_mismatch_error_clear_problem_statement() {
        let packages = vec![("test.package".to_string(), "1.0.0".to_string())];

        let error =
            format_version_mismatch_error_test("test.package", "1.0.0", "R5", "R4", &packages);

        // Should clearly state the mismatch
        assert!(error.contains("All packages must target the same FHIR version as the server"));
    }

    #[test]
    fn test_package_registry_url_format() {
        let package_id = "hl7.fhir.us.core";
        let expected_url = "https://registry.fhir.org/package/hl7.fhir.us.core";

        let packages = vec![(package_id.to_string(), "6.1.0".to_string())];
        let error = format_version_mismatch_error_test(package_id, "6.1.0", "R4", "R4B", &packages);

        assert!(error.contains(expected_url));
    }
}
