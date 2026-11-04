use octofhir_search::terminology_preprocess::{
    TerminologyPreprocessError, pre_expand_subsumption_modifiers,
};
use octofhir_search::{
    HierarchyDirection, HierarchyProvider, SearchParameterRegistry, parse_search_parameter,
};
use octofhir_storage::SearchParams;
use std::sync::Arc;

struct EmbeddedHierarchy;
#[async_trait::async_trait]
impl HierarchyProvider for EmbeddedHierarchy {
    async fn expand_hierarchy(
        &self,
        system: &str,
        code: &str,
        direction: HierarchyDirection,
    ) -> Result<Vec<String>, String> {
        assert_eq!((system, code), ("urn:test", "parent"));
        assert_eq!(direction, HierarchyDirection::Below);
        Ok(vec!["child".into(), "parent".into()])
    }
}

#[tokio::test]
async fn local_hierarchy_expansion_preserves_search_semantics_and_limits() {
    let registry = SearchParameterRegistry::new();
    registry.register(parse_search_parameter(&serde_json::json!({
        "resourceType": "SearchParameter", "url": "urn:test:code", "code": "code", "base": ["Observation"],
        "type": "token", "expression": "Observation.code"
    })).unwrap());
    let provider: Arc<dyn HierarchyProvider> = Arc::new(EmbeddedHierarchy);
    let original = SearchParams::new().with_param("code:below", "urn:test|parent");
    let mut params = original.clone();
    pre_expand_subsumption_modifiers(&mut params, &registry, "Observation", &provider, 10)
        .await
        .unwrap();
    assert!(!params.parameters.contains_key("code:below"));
    assert_eq!(
        params.parameters["code"],
        ["urn:test|child,urn:test|parent"]
    );
    let mut params = original;
    assert!(matches!(
        pre_expand_subsumption_modifiers(&mut params, &registry, "Observation", &provider, 1).await,
        Err(TerminologyPreprocessError::ExpansionTooLarge { .. })
    ));
}
