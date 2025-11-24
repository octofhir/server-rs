//! Database synchronization for Canonical Manager.
//!
//! This module provides functionality to sync conformance resources from PostgreSQL
//! to the canonical manager's in-memory index.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tracing::{debug, info, instrument, warn};

use octofhir_storage::{ConformanceStorage, StorageError};

use crate::conformance::PostgresConformanceStorage;

/// Synchronizes conformance resources from PostgreSQL to a file-based directory.
///
/// This function loads all conformance resources (StructureDefinitions, ValueSets,
/// CodeSystems, SearchParameters) from the database and writes them to a directory
/// structure compatible with the canonical manager.
///
/// # Directory Structure
///
/// ```text
/// {output_dir}/
///   package.json
///   StructureDefinition-{id}.json
///   ValueSet-{id}.json
///   CodeSystem-{id}.json
///   SearchParameter-{id}.json
/// ```
///
/// # Arguments
///
/// * `conformance_storage` - The PostgreSQL conformance storage to read from
/// * `output_dir` - Directory to write the conformance resources
/// * `package_name` - Name for the package.json (e.g., "octofhir.internal")
/// * `package_version` - Version for the package.json (e.g., "0.1.0")
///
/// # Errors
///
/// Returns an error if database queries fail or file writes fail.
#[instrument(skip(conformance_storage))]
pub async fn sync_to_directory(
    conformance_storage: &PostgresConformanceStorage,
    output_dir: &Path,
    package_name: &str,
    package_version: &str,
) -> Result<SyncStats, StorageError> {
    info!(
        output_dir = %output_dir.display(),
        package_name,
        package_version,
        "Starting conformance resource sync"
    );

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .map_err(|e| StorageError::internal(format!("Failed to create output directory: {}", e)))?;

    // Load all conformance resources from database
    let (structure_definitions, value_sets, code_systems, search_parameters) =
        conformance_storage.load_all_conformance().await?;

    let mut stats = SyncStats::default();
    stats.structure_definitions = structure_definitions.len();
    stats.value_sets = value_sets.len();
    stats.code_systems = code_systems.len();
    stats.search_parameters = search_parameters.len();

    // Write StructureDefinitions
    for sd in structure_definitions {
        let filename = generate_filename(&sd, "StructureDefinition");
        let filepath = output_dir.join(&filename);
        write_json_file(&filepath, &sd)?;
        debug!("Wrote {}", filename);
    }

    // Write ValueSets
    for vs in value_sets {
        let filename = generate_filename(&vs, "ValueSet");
        let filepath = output_dir.join(&filename);
        write_json_file(&filepath, &vs)?;
        debug!("Wrote {}", filename);
    }

    // Write CodeSystems
    for cs in code_systems {
        let filename = generate_filename(&cs, "CodeSystem");
        let filepath = output_dir.join(&filename);
        write_json_file(&filepath, &cs)?;
        debug!("Wrote {}", filename);
    }

    // Write SearchParameters
    for sp in search_parameters {
        let filename = generate_filename(&sp, "SearchParameter");
        let filepath = output_dir.join(&filename);
        write_json_file(&filepath, &sp)?;
        debug!("Wrote {}", filename);
    }

    // Write package.json
    write_package_json(output_dir, package_name, package_version)?;

    info!(
        structure_definitions = stats.structure_definitions,
        value_sets = stats.value_sets,
        code_systems = stats.code_systems,
        search_parameters = stats.search_parameters,
        total = stats.total(),
        "Conformance resource sync completed"
    );

    Ok(stats)
}

/// Loads conformance resources from PostgreSQL into an in-memory collection.
///
/// This is useful for canonical manager implementations that support
/// in-memory resource loading without filesystem persistence.
#[instrument(skip(conformance_storage))]
pub async fn load_to_memory(
    conformance_storage: &PostgresConformanceStorage,
) -> Result<ConformanceBundle, StorageError> {
    let (structure_definitions, value_sets, code_systems, search_parameters) =
        conformance_storage.load_all_conformance().await?;

    Ok(ConformanceBundle {
        structure_definitions,
        value_sets,
        code_systems,
        search_parameters,
    })
}

/// Statistics about a conformance sync operation.
#[derive(Debug, Default, Clone)]
pub struct SyncStats {
    pub structure_definitions: usize,
    pub value_sets: usize,
    pub code_systems: usize,
    pub search_parameters: usize,
}

impl SyncStats {
    /// Returns the total number of synced resources.
    pub fn total(&self) -> usize {
        self.structure_definitions + self.value_sets + self.code_systems + self.search_parameters
    }
}

/// Bundle of conformance resources loaded from the database.
#[derive(Debug, Clone)]
pub struct ConformanceBundle {
    pub structure_definitions: Vec<Value>,
    pub value_sets: Vec<Value>,
    pub code_systems: Vec<Value>,
    pub search_parameters: Vec<Value>,
}

impl ConformanceBundle {
    /// Returns the total number of resources in this bundle.
    pub fn total(&self) -> usize {
        self.structure_definitions.len()
            + self.value_sets.len()
            + self.code_systems.len()
            + self.search_parameters.len()
    }
}

/// Generates a filename for a conformance resource.
///
/// Format: `{ResourceType}-{id}.json`
fn generate_filename(resource: &Value, resource_type: &str) -> String {
    let id = resource
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    format!("{}-{}.json", resource_type, id)
}

/// Writes a JSON resource to a file.
fn write_json_file(path: &Path, resource: &Value) -> Result<(), StorageError> {
    let json_str = serde_json::to_string_pretty(resource)
        .map_err(|e| StorageError::internal(format!("Failed to serialize JSON: {}", e)))?;

    std::fs::write(path, json_str)
        .map_err(|e| StorageError::internal(format!("Failed to write file: {}", e)))?;

    Ok(())
}

/// Writes a package.json file for the conformance package.
fn write_package_json(
    dir: &Path,
    package_name: &str,
    package_version: &str,
) -> Result<(), StorageError> {
    let package_json = serde_json::json!({
        "name": package_name,
        "version": package_version,
        "fhirVersion": "4.0.1",  // Default to R4; could be configurable
        "type": "fhir.ig",
        "description": "OctoFHIR internal conformance resources",
        "author": "OctoFHIR",
        "url": format!("http://octofhir.io/ig/{}", package_name),
        "dependencies": {
            "hl7.fhir.r4.core": "4.0.1"
        }
    });

    let package_path = dir.join("package.json");
    write_json_file(&package_path, &package_json)?;
    debug!("Wrote package.json");

    Ok(())
}

/// Helper function to sync conformance resources and load them into canonical manager.
///
/// This is a convenience function that combines syncing to a directory and
/// loading via canonical manager.
///
/// # Arguments
///
/// * `conformance_storage` - PostgreSQL conformance storage
/// * `base_dir` - Base directory for canonical manager packages
/// * `manager` - Canonical manager instance (optional)
///
/// # Returns
///
/// The path to the synced package directory.
#[instrument(skip(conformance_storage, manager))]
pub async fn sync_and_load(
    conformance_storage: &PostgresConformanceStorage,
    base_dir: &Path,
    manager: Option<&Arc<octofhir_canonical_manager::CanonicalManager>>,
) -> Result<PathBuf, StorageError> {
    const PACKAGE_NAME: &str = "octofhir.internal";
    const PACKAGE_VERSION: &str = "0.1.0";

    // Create output directory for internal package
    let package_dir = base_dir.join("octofhir-internal");

    // Sync to directory
    let stats = sync_to_directory(
        conformance_storage,
        &package_dir,
        PACKAGE_NAME,
        PACKAGE_VERSION,
    )
    .await?;

    info!(
        package_dir = %package_dir.display(),
        total_resources = stats.total(),
        "Synced internal conformance resources"
    );

    // If manager is provided, load the package
    if let Some(mgr) = manager {
        match mgr
            .load_from_directory(&package_dir, PACKAGE_NAME, PACKAGE_VERSION)
            .await
        {
            Ok(_) => {
                info!(
                    package_name = PACKAGE_NAME,
                    package_version = PACKAGE_VERSION,
                    "Loaded internal package into canonical manager"
                );
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to load internal package into canonical manager"
                );
            }
        }
    }

    Ok(package_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_filename() {
        let resource = serde_json::json!({
            "resourceType": "StructureDefinition",
            "id": "App",
            "url": "http://octofhir.io/StructureDefinition/App"
        });

        let filename = generate_filename(&resource, "StructureDefinition");
        assert_eq!(filename, "StructureDefinition-App.json");
    }

    #[test]
    fn test_sync_stats_total() {
        let stats = SyncStats {
            structure_definitions: 2,
            value_sets: 3,
            code_systems: 1,
            search_parameters: 4,
        };

        assert_eq!(stats.total(), 10);
    }
}
