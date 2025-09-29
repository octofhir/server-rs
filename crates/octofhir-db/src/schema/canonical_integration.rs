// Integration with octofhir-canonical-manager for StructureDefinition retrieval
// Provides a bridge between canonical manager search and schema generation

use super::fhir_parser::parse_structure_definition;
use super::generator::{GeneratedSchema, SchemaGenerator};
use octofhir_canonical_manager::CanonicalManager;
use std::sync::Arc;

/// Schema manager that integrates with Canonical Manager
pub struct CanonicalSchemaManager {
    canonical_manager: Arc<CanonicalManager>,
    schema_generator: SchemaGenerator,
}

impl CanonicalSchemaManager {
    pub fn new(canonical_manager: Arc<CanonicalManager>) -> Self {
        Self {
            canonical_manager,
            schema_generator: SchemaGenerator::new(),
        }
    }

    pub fn with_generator(mut self, generator: SchemaGenerator) -> Self {
        self.schema_generator = generator;
        self
    }

    /// Generate schema for a FHIR resource type from a StructureDefinition JSON string
    ///
    /// # Arguments
    /// * `resource_type` - The FHIR resource type (e.g., "Patient", "Observation")
    /// * `sd_json` - The StructureDefinition JSON content
    ///
    /// # Returns
    /// Generated schema with DDL, or an error if parsing/generation failed
    pub fn generate_schema_from_json(
        &self,
        resource_type: &str,
        sd_json: &str,
    ) -> Result<GeneratedSchema, SchemaManagerError> {
        // Parse the StructureDefinition
        let elements = parse_structure_definition(sd_json)
            .map_err(|e| SchemaManagerError::ParseFailed(e.to_string()))?;

        // Generate schema
        let schema = self
            .schema_generator
            .generate_resource_schema(resource_type, elements)
            .map_err(|e| SchemaManagerError::GenerationFailed(e.to_string()))?;

        Ok(schema)
    }

    /// Generate schema for a FHIR resource type by querying the canonical manager
    ///
    /// This is a placeholder for future integration. The canonical manager API
    /// is still evolving. For now, use `generate_schema_from_json` with manually
    /// retrieved StructureDefinition JSON.
    ///
    /// # Arguments
    /// * `resource_type` - The FHIR resource type (e.g., "Patient", "Observation")
    ///
    /// # Returns
    /// Generated schema with DDL, or an error if StructureDefinition not found or parsing failed
    pub async fn generate_schema_for_resource(
        &self,
        resource_type: &str,
    ) -> Result<GeneratedSchema, SchemaManagerError> {
        // TODO: Once canonical manager API is stable, implement proper querying
        // For now, return a helpful error message
        Err(SchemaManagerError::CanonicalQueryFailed(format!(
            "Canonical manager integration pending. Use generate_schema_from_json() with StructureDefinition for '{}'",
            resource_type
        )))
    }

    /// Generate schemas for multiple resource types in bulk
    pub async fn generate_schemas_bulk(
        &self,
        resource_types: &[&str],
    ) -> Vec<Result<GeneratedSchema, SchemaManagerError>> {
        let mut results = Vec::new();

        for resource_type in resource_types {
            results.push(self.generate_schema_for_resource(resource_type).await);
        }

        results
    }

    /// List all available StructureDefinition resources in the canonical manager
    ///
    /// Placeholder for future implementation when canonical manager API is stable.
    pub async fn list_available_resources(&self) -> Result<Vec<String>, SchemaManagerError> {
        // TODO: Implement once canonical manager search API is finalized
        Err(SchemaManagerError::CanonicalQueryFailed(
            "List functionality pending canonical manager API updates".to_string(),
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaManagerError {
    #[error("Failed to query canonical manager: {0}")]
    CanonicalQueryFailed(String),

    #[error("StructureDefinition not found for resource type: {0}")]
    StructureDefinitionNotFound(String),

    #[error("Failed to retrieve resource content: {0}")]
    ResourceRetrievalFailed(String),

    #[error("Failed to parse StructureDefinition: {0}")]
    ParseFailed(String),

    #[error("Failed to generate schema: {0}")]
    GenerationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running canonical manager with FHIR packages installed
    // They are integration tests and may be slower or require setup

    #[tokio::test]
    #[ignore] // Ignore by default since it requires canonical manager setup
    async fn test_canonical_schema_manager_integration() {
        use octofhir_canonical_manager::FcmConfig;

        // Setup canonical manager
        let mut config = FcmConfig::default();
        config.add_package("hl7.fhir.r5.core", "5.0.0", Some(1));

        let manager = Arc::new(CanonicalManager::new(config).await.unwrap());
        manager.install_package("hl7.fhir.r5.core", "5.0.0").await.unwrap();
        manager.force_full_rebuild().await.unwrap();

        // Create schema manager
        let schema_mgr = CanonicalSchemaManager::new(manager);

        // Generate schema for Patient
        let schema = schema_mgr
            .generate_schema_for_resource("Patient")
            .await
            .expect("Failed to generate Patient schema");

        assert_eq!(schema.resource_type, "Patient");
        assert!(schema.base_ddl.contains("patient"));
    }
}