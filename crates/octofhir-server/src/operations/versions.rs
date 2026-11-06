//! `$versions`: the FHIR versions of this endpoint. A server runs one.

use async_trait::async_trait;
use octofhir_core::fhir::FhirVersion;
use serde_json::{Value, json};

use super::handler::{OperationError, OperationHandler, OperationLevel};
use crate::server::AppState;

pub struct VersionsOperation;

/// `major.minor` of a version, as the operation reports it.
fn major_minor(version: &str) -> &str {
    let end = version
        .match_indices('.')
        .nth(1)
        .map_or(version.len(), |(index, _)| index);
    &version[..end]
}

#[async_trait]
impl OperationHandler for VersionsOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "versions"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        _context: &super::OperationContext,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        let release: FhirVersion =
            state
                .fhir_version
                .parse()
                .map_err(|error: octofhir_core::CoreError| {
                    OperationError::Internal(error.to_string())
                })?;
        let version = major_minor(release.version());
        Ok(json!({"resourceType": "Parameters", "parameter": [
            {"name": "version", "valueCode": version},
            {"name": "default", "valueCode": version},
        ]}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_major_and_minor() {
        assert_eq!(major_minor("4.0.1"), "4.0");
        assert_eq!(major_minor("6.0.0-snapshot1"), "6.0");
        assert_eq!(major_minor("4.3"), "4.3");
    }
}
