use serde::{Deserialize, Serialize};

/// Physical search strategy selected for a typed FHIR predicate.
///
/// All predicates render in-place over the resource JSONB:
/// - `JsonbExpressionIndex`: predicate over a functional expression that a
///   bootstrap-created functional index (GiST/GIN) matches exactly.
/// - `JsonbContainment`: `resource @> $jsonb`, served by the per-table
///   `GIN (resource jsonb_path_ops)` index.
/// - `JsonbTraversal`: plain JSONB path extraction/casts; not index-backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexStrategy {
    JsonbContainment,
    JsonbTraversal,
    JsonbExpressionIndex,
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
    /// Predicate over a functional expression matched by a bootstrap-created
    /// functional index (e.g. `idx_{table}_{param}_date` GiST,
    /// `idx_{table}_{param}_str` trigram GIN).
    pub fn jsonb_expression_index(
        expected_index: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            strategy: IndexStrategy::JsonbExpressionIndex,
            expected_index: Some(expected_index.into()),
            index_backed: true,
            reason: reason.into(),
        }
    }

    /// `resource @> $jsonb` containment served by the per-table
    /// `GIN (resource jsonb_path_ops)` index (`idx_{table}_gin`).
    pub fn jsonb_containment(expected_index: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            strategy: IndexStrategy::JsonbContainment,
            expected_index: Some(expected_index.into()),
            index_backed: true,
            reason: reason.into(),
        }
    }

    /// Plain in-place JSONB traversal (path extraction / casts); not
    /// index-backed.
    pub fn jsonb_traversal(reason: impl Into<String>) -> Self {
        Self {
            strategy: IndexStrategy::JsonbTraversal,
            expected_index: None,
            index_backed: false,
            reason: reason.into(),
        }
    }
}
