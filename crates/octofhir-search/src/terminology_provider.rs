//! Transport-independent terminology hierarchy expansion.

/// Direction for hierarchy traversal in subsumption searches.
///
/// Used for `:below` and `:above` modifiers in token searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyDirection {
    /// Descendants (subsumes) - used for `:below` modifier
    /// Finds all codes that are subsumed by (descendants of) the given code
    Below,

    /// Ancestors (subsumed-by) - used for `:above` modifier
    /// Finds all codes that subsume (ancestors of) the given code
    Above,
}

/// Expands ancestors or descendants using a caller-provided terminology source.
#[async_trait::async_trait]
pub trait HierarchyProvider: Send + Sync {
    /// Returns related codes, or an explanatory error. Callers add the seed code.
    async fn expand_hierarchy(
        &self,
        system: &str,
        code: &str,
        direction: HierarchyDirection,
    ) -> Result<Vec<String>, String>;
}
