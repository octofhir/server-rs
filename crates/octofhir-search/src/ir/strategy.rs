use serde::{Deserialize, Serialize};

/// Physical search strategy selected for a typed FHIR predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexStrategy {
    JsonbContainment,
    JsonbTraversal,
    JsonbExpressionIndex,
    SidecarDate,
    SidecarReference,
    SidecarString,
    SidecarToken,
    SidecarNumber,
    SidecarQuantity,
    SidecarUri,
    SidecarComposite,
    GeneratedColumn,
    Disabled,
}

/// Explainable result of strategy selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyDecision {
    pub strategy: IndexStrategy,
    pub expected_index: Option<String>,
    pub index_backed: bool,
    pub reason: String,
}

impl StrategyDecision {
    pub fn sidecar_date() -> Self {
        Self {
            strategy: IndexStrategy::SidecarDate,
            expected_index: Some("search_idx_date_*_param_code_rng_idx".to_string()),
            index_backed: true,
            reason: "date search uses search_idx_date tstzrange GiST".to_string(),
        }
    }

    pub fn sidecar_string() -> Self {
        Self {
            strategy: IndexStrategy::SidecarString,
            expected_index: Some("search_idx_string_*_param_code_value_norm_trgm_idx".to_string()),
            index_backed: true,
            reason: "string search uses search_idx_string normalized text indexes".to_string(),
        }
    }
}
