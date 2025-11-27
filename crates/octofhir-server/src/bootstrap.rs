//! Bootstrap module for loading internal conformance resources.
//!
//! This module loads StructureDefinitions, ValueSets, and CodeSystems from
//! embedded resources into the database on first startup.
//!
//! Resources are embedded at compile time using `include_str!` for single-binary distribution.

use octofhir_db_postgres::PostgresConformanceStorage;
use octofhir_storage::ConformanceStorage;
use tracing::{info, warn};

/// Embedded internal IG resources
/// These are compiled into the binary for single-binary distribution
const EMBEDDED_RESOURCES: &[(&str, &str)] = &[
    // StructureDefinitions - Gateway
    (
        "StructureDefinition-App.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-App.json"),
    ),
    (
        "StructureDefinition-CustomOperation.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-CustomOperation.json"),
    ),
    // StructureDefinitions - Auth
    (
        "StructureDefinition-Client.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-Client.json"),
    ),
    (
        "StructureDefinition-User.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-User.json"),
    ),
    (
        "StructureDefinition-AccessPolicy.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-AccessPolicy.json"),
    ),
    (
        "StructureDefinition-Session.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-Session.json"),
    ),
    (
        "StructureDefinition-RefreshToken.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-RefreshToken.json"),
    ),
    // ValueSets
    (
        "ValueSet-http-methods.json",
        include_str!("../../../igs/octofhir-internal/ValueSet-http-methods.json"),
    ),
    (
        "ValueSet-operation-types.json",
        include_str!("../../../igs/octofhir-internal/ValueSet-operation-types.json"),
    ),
    // CodeSystems
    (
        "CodeSystem-http-methods.json",
        include_str!("../../../igs/octofhir-internal/CodeSystem-http-methods.json"),
    ),
    (
        "CodeSystem-operation-types.json",
        include_str!("../../../igs/octofhir-internal/CodeSystem-operation-types.json"),
    ),
];

/// Bootstraps conformance resources from embedded resources into the database.
///
/// This function:
/// 1. Checks if resources already exist (idempotent)
/// 2. Loads embedded JSON resources (compiled into binary)
/// 3. Inserts StructureDefinitions, ValueSets, and CodeSystems
///
/// # Errors
///
/// Returns an error if:
/// - JSON files are malformed
/// - Database operations fail
pub async fn bootstrap_conformance_resources(
    conformance_storage: &PostgresConformanceStorage,
) -> Result<BootstrapStats, Box<dyn std::error::Error>> {
    info!("Starting conformance resource bootstrap from embedded resources");

    let mut stats = BootstrapStats::default();

    // Check if already bootstrapped (check for App StructureDefinition)
    if let Ok(Some(_)) = conformance_storage
        .get_structure_definition_by_url("http://octofhir.io/StructureDefinition/App", None)
        .await
    {
        info!("Conformance resources already bootstrapped, skipping");
        return Ok(stats);
    }

    info!(
        "Loading {} embedded conformance resources",
        EMBEDDED_RESOURCES.len()
    );

    // Load all embedded resources
    for (filename, content) in EMBEDDED_RESOURCES {
        // Parse the resource
        let resource: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse {}: {}", filename, e))?;

        let resource_type = resource["resourceType"]
            .as_str()
            .ok_or_else(|| format!("Missing resourceType in {}", filename))?;

        let name = resource["name"].as_str().unwrap_or("unknown");

        // Insert based on resource type
        match resource_type {
            "StructureDefinition" => {
                conformance_storage
                    .create_structure_definition(&resource)
                    .await?;
                info!("Loaded StructureDefinition: {}", name);
                stats.structure_definitions += 1;
            }
            "ValueSet" => {
                conformance_storage.create_value_set(&resource).await?;
                info!("Loaded ValueSet: {}", name);
                stats.value_sets += 1;
            }
            "CodeSystem" => {
                conformance_storage.create_code_system(&resource).await?;
                info!("Loaded CodeSystem: {}", name);
                stats.code_systems += 1;
            }
            "SearchParameter" => {
                conformance_storage
                    .create_search_parameter(&resource)
                    .await?;
                info!("Loaded SearchParameter: {}", name);
                stats.search_parameters += 1;
            }
            other => {
                warn!("Skipping unsupported resource type: {}", other);
            }
        }
    }

    info!(
        structure_definitions = stats.structure_definitions,
        value_sets = stats.value_sets,
        code_systems = stats.code_systems,
        search_parameters = stats.search_parameters,
        total = stats.total(),
        "Conformance bootstrap completed"
    );

    Ok(stats)
}

/// Statistics about the bootstrap operation.
#[derive(Debug, Default)]
pub struct BootstrapStats {
    pub structure_definitions: usize,
    pub value_sets: usize,
    pub code_systems: usize,
    pub search_parameters: usize,
}

impl BootstrapStats {
    /// Returns the total number of resources loaded.
    pub fn total(&self) -> usize {
        self.structure_definitions + self.value_sets + self.code_systems + self.search_parameters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_stats_total() {
        let stats = BootstrapStats {
            structure_definitions: 2,
            value_sets: 2,
            code_systems: 2,
            search_parameters: 0,
        };

        assert_eq!(stats.total(), 6);
    }
}
