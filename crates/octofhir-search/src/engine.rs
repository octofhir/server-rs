use crate::parser::{SearchParameterParser, SearchValidationError};
use octofhir_core::ResourceType;
use octofhir_db::{DynStorage, QueryResult};
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

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::{ResourceEnvelope, ResourceType};
    use octofhir_db::{StorageConfig as DbStorageConfig, create_storage};
    use serde_json::json;
    use tokio_test::block_on;

    fn make_patient(id: &str, name: &str, mrn: &str) -> ResourceEnvelope {
        let mut r = ResourceEnvelope::new(id.to_string(), ResourceType::Patient);
        r.add_field(
            "name".to_string(),
            json!([{ "family": name, "given": ["John"] }]),
        );
        r.add_field(
            "identifier".to_string(),
            json!([{ "system": "http://sys", "value": mrn }]),
        );
        r
    }

    #[test]
    fn engine_filters_and_paginates_and_sorts() {
        let storage = create_storage(&DbStorageConfig::default());
        block_on(async {
            // insert resources
            storage
                .insert(&ResourceType::Patient, make_patient("a1", "Alpha", "1"))
                .await
                .unwrap();
            storage
                .insert(&ResourceType::Patient, make_patient("b2", "Beta", "2"))
                .await
                .unwrap();
            storage
                .insert(&ResourceType::Patient, make_patient("c3", "Gamma", "3"))
                .await
                .unwrap();

            let cfg = SearchConfig::default();

            // filter by name contains
            let res = SearchEngine::execute(&storage, ResourceType::Patient, "name=Al", &cfg)
                .await
                .unwrap();
            assert_eq!(res.total, 1);
            assert_eq!(res.resources.len(), 1);
            assert_eq!(res.resources[0].id, "a1");

            // sort desc by _id and paginate
            let res = SearchEngine::execute(
                &storage,
                ResourceType::Patient,
                "_sort=-_id&_count=1&_offset=1",
                &cfg,
            )
            .await
            .unwrap();
            assert_eq!(res.total, 3);
            assert_eq!(res.resources.len(), 1);
            // sorted ids: c3, b2, a1 → offset 1 gives b2
            assert_eq!(res.resources[0].id, "b2");
        });
    }

    #[test]
    fn engine_validation_errors() {
        let storage = create_storage(&DbStorageConfig::default());
        let cfg = SearchConfig::default();
        // unknown param
        let err = block_on(SearchEngine::execute(
            &storage,
            ResourceType::Patient,
            "foo=bar",
            &cfg,
        ))
        .unwrap_err();
        assert!(matches!(err, EngineError::Validation(_)));
        // invalid _count
        let err = block_on(SearchEngine::execute(
            &storage,
            ResourceType::Patient,
            "_count=0",
            &cfg,
        ))
        .unwrap_err();
        assert!(matches!(err, EngineError::Validation(_)));
    }

    #[test]
    fn engine_multi_param_and_and_offset_beyond_total() {
        let storage = create_storage(&DbStorageConfig::default());
        block_on(async {
            // Insert patients
            storage
                .insert(&ResourceType::Patient, make_patient("p1", "Smith", "A1"))
                .await
                .unwrap();
            storage
                .insert(&ResourceType::Patient, make_patient("p2", "Smythe", "A1"))
                .await
                .unwrap();
            storage
                .insert(&ResourceType::Patient, make_patient("p3", "Smith", "B2"))
                .await
                .unwrap();

            let cfg = SearchConfig::default();
            // name contains 'Smi' AND identifier system=http://sys|A1 (we only support value in Identifier filter here)
            let res = SearchEngine::execute(
                &storage,
                ResourceType::Patient,
                "name=Smi&identifier=http://sys|A1",
                &cfg,
            )
            .await
            .unwrap();
            // p1 matches (Smith + A1); p2 fails name contains; p3 fails identifier value
            assert_eq!(res.total, 1);
            assert_eq!(res.resources.len(), 1);
            assert_eq!(res.resources[0].id, "p1");

            // Offset beyond total should return empty page with total intact
            let res2 = SearchEngine::execute(
                &storage,
                ResourceType::Patient,
                "name=Smi&identifier=http://sys|A1&_offset=10&_count=5",
                &cfg,
            )
            .await
            .unwrap();
            assert_eq!(res2.total, 1);
            assert_eq!(res2.resources.len(), 0);
        });
    }

    #[ignore]
    #[test]
    fn performance_engine_scales() {
        let storage = create_storage(&DbStorageConfig::default());
        block_on(async {
            for i in 0..2000 {
                let id = format!("p{i}");
                let name = if i % 2 == 0 { "Even" } else { "Odd" };
                let mrn = format!("{i}");
                let _ = storage
                    .insert(&ResourceType::Patient, make_patient(&id, name, &mrn))
                    .await;
            }
            let cfg = SearchConfig::default();
            let res = SearchEngine::execute(
                &storage,
                ResourceType::Patient,
                "name=Even&_count=50&_offset=100",
                &cfg,
            )
            .await
            .unwrap();
            assert!(res.total >= 1000);
            assert_eq!(res.count, 50);
        });
    }
}
