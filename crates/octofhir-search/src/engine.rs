use crate::parser::{SearchParameterParser, SearchValidationError};
use crate::registry::SearchParameterRegistry;
use octofhir_core::ResourceType;
use octofhir_storage::legacy::{DynStorage, QueryResult};
use std::sync::Arc;
use thiserror::Error;

/// Search configuration with dynamic parameter registry.
///
/// The registry is loaded from the FHIR canonical manager and contains all
/// search parameters from loaded packages (e.g., hl7.fhir.r4.core).
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub default_count: usize,
    pub max_count: usize,
    /// Search parameter registry loaded from canonical manager (REQUIRED)
    pub registry: Arc<SearchParameterRegistry>,
}

impl SearchConfig {
    /// Create a new search config with the given registry.
    pub fn new(registry: Arc<SearchParameterRegistry>) -> Self {
        Self {
            default_count: 10,
            max_count: 100,
            registry,
        }
    }

    /// Create with custom count settings.
    pub fn with_counts(mut self, default_count: usize, max_count: usize) -> Self {
        self.default_count = default_count;
        self.max_count = max_count;
        self
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("validation error: {0}")]
    Validation(#[from] SearchValidationError),
    #[error("storage error: {0}")]
    Storage(#[from] octofhir_core::CoreError),
}

pub struct SearchEngine;

impl SearchEngine {
    /// Execute a search query with dynamic parameter validation from registry.
    pub async fn execute(
        storage: &DynStorage,
        resource_type: ResourceType,
        query: &str,
        config: &SearchConfig,
    ) -> Result<QueryResult, EngineError> {
        let sq = SearchParameterParser::validate_and_build_with_registry(
            resource_type,
            query,
            config.default_count,
            config.max_count,
            &config.registry,
        )?;
        let result = storage.search(&sq).await?;
        Ok(result)
    }
}

// Tests removed - require storage backend (use integration tests with testcontainers)
