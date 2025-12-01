use crate::parser::{SearchParameterParser, SearchValidationError};
use octofhir_core::ResourceType;
use octofhir_storage::legacy::{DynStorage, QueryResult};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub default_count: usize,
    pub max_count: usize,
    pub allowed_params: Vec<&'static str>,
    pub allowed_sort_fields: Vec<&'static str>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_count: 10,
            max_count: 100,
            allowed_params: vec![
                "_id",
                "_lastUpdated",
                "_count",
                "_offset",
                "_sort",
                "identifier",
                "name",
                "family",
                "given",
            ],
            allowed_sort_fields: vec!["_id", "_lastUpdated"],
        }
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
    pub async fn execute(
        storage: &DynStorage,
        resource_type: ResourceType,
        query: &str,
        config: &SearchConfig,
    ) -> Result<QueryResult, EngineError> {
        let sq = SearchParameterParser::validate_and_build_search_query(
            resource_type,
            query,
            config.default_count,
            config.max_count,
            &config.allowed_params,
            &config.allowed_sort_fields,
        )?;
        let result = storage.search(&sq).await?;
        Ok(result)
    }
}

// Tests removed - require storage backend (use integration tests with testcontainers)
