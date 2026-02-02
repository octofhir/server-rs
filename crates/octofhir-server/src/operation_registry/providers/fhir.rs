//! FHIR REST API Operation Provider

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

const FHIR_BASE: &str = "/fhir";

fn fhir_path(suffix: &str) -> String {
    format!("{FHIR_BASE}{suffix}")
}

/// Provider for FHIR REST API operations
pub struct FhirOperationProvider;

impl OperationProvider for FhirOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // Read operations
            OperationDefinition::new(
                "fhir.read",
                "Read Resource",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/{id}"),
                modules::SERVER,
            )
            .with_description("Read a single FHIR resource by ID"),
            OperationDefinition::new(
                "fhir.vread",
                "Version Read",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/{id}/_history/{vid}"),
                modules::SERVER,
            )
            .with_description("Read a specific version of a FHIR resource"),
            // Create/Update/Delete
            OperationDefinition::new(
                "fhir.create",
                "Create Resource",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}"),
                modules::SERVER,
            )
            .with_description("Create a new FHIR resource"),
            OperationDefinition::new(
                "fhir.update",
                "Update Resource",
                categories::FHIR,
                vec!["PUT".to_string()],
                fhir_path("/{type}/{id}"),
                modules::SERVER,
            )
            .with_description("Update an existing FHIR resource"),
            OperationDefinition::new(
                "fhir.patch",
                "Patch Resource",
                categories::FHIR,
                vec!["PATCH".to_string()],
                fhir_path("/{type}/{id}"),
                modules::SERVER,
            )
            .with_description("Apply a partial update to a FHIR resource"),
            OperationDefinition::new(
                "fhir.delete",
                "Delete Resource",
                categories::FHIR,
                vec!["DELETE".to_string()],
                fhir_path("/{type}/{id}"),
                modules::SERVER,
            )
            .with_description("Delete a FHIR resource"),
            // Search operations
            OperationDefinition::new(
                "fhir.search",
                "Search Resources",
                categories::FHIR,
                vec!["GET".to_string(), "POST".to_string()],
                fhir_path("/{type}"),
                modules::SERVER,
            )
            .with_description("Search for FHIR resources by parameters"),
            OperationDefinition::new(
                "fhir.search-all",
                "Search All Resources",
                categories::FHIR,
                vec!["GET".to_string(), "POST".to_string()],
                fhir_path(""),
                modules::SERVER,
            )
            .with_description("Search across all resource types"),
            // History operations
            OperationDefinition::new(
                "fhir.history-instance",
                "Instance History",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/{id}/_history"),
                modules::SERVER,
            )
            .with_description("Get the history of a specific resource"),
            OperationDefinition::new(
                "fhir.history-type",
                "Type History",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/_history"),
                modules::SERVER,
            )
            .with_description("Get the history of all resources of a type"),
            OperationDefinition::new(
                "fhir.history-system",
                "System History",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/_history"),
                modules::SERVER,
            )
            .with_description("Get the history of all resources in the system"),
            // Batch/Transaction
            OperationDefinition::new(
                "fhir.batch",
                "Batch",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path(""),
                modules::SERVER,
            )
            .with_description("Execute a batch of independent operations"),
            OperationDefinition::new(
                "fhir.transaction",
                "Transaction",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path(""),
                modules::SERVER,
            )
            .with_description("Execute a transaction with atomic semantics"),
            // FHIR Operations (extended operations)
            OperationDefinition::new(
                "fhir.validate",
                "$validate",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}/$validate"),
                modules::SERVER,
            )
            .with_description("Validate a FHIR resource"),
            OperationDefinition::new(
                "fhir.everything",
                "$everything",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/{id}/$everything"),
                modules::SERVER,
            )
            .with_description("Get the complete record for Patient, Encounter, or Group"),
            OperationDefinition::new(
                "fhir.meta",
                "$meta",
                categories::FHIR,
                vec!["GET".to_string()],
                fhir_path("/{type}/{id}/$meta"),
                modules::SERVER,
            )
            .with_description("Get resource metadata"),
            OperationDefinition::new(
                "fhir.meta-add",
                "$meta-add",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}/{id}/$meta-add"),
                modules::SERVER,
            )
            .with_description("Add metadata elements"),
            OperationDefinition::new(
                "fhir.meta-delete",
                "$meta-delete",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}/{id}/$meta-delete"),
                modules::SERVER,
            )
            .with_description("Remove metadata elements"),
            OperationDefinition::new(
                "fhir.fhirpath",
                "$fhirpath",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/$fhirpath"),
                modules::SERVER,
            )
            .with_description("Evaluate FHIRPath expressions against FHIR resources"),
            OperationDefinition::new(
                "fhir.fhirpath-type",
                "$fhirpath",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}/$fhirpath"),
                modules::SERVER,
            )
            .with_description("Evaluate FHIRPath expressions for a resource type"),
            OperationDefinition::new(
                "fhir.fhirpath-instance",
                "$fhirpath",
                categories::FHIR,
                vec!["POST".to_string()],
                fhir_path("/{type}/{id}/$fhirpath"),
                modules::SERVER,
            )
            .with_description("Evaluate FHIRPath expressions on a resource instance"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::SERVER
    }
}
