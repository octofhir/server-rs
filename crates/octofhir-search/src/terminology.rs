//! Hybrid Terminology Provider for FHIR search operations.
//!
//! Provides a two-tier terminology lookup strategy:
//! 1. Local FHIR packages via CanonicalManager - fast, no network
//! 2. Remote terminology server with moka caching - network latency, cached
//!
//! Caching is handled by `CachedTerminologyProvider<HttpTerminologyProvider>` from
//! `octofhir-fhir-model`, which provides TTL-based caching using the moka library.

use async_trait::async_trait;
use octofhir_canonical_manager::CanonicalManager;
use octofhir_fhir_model::terminology::{
    ConnectionStatus, ExpansionParameters, HttpTerminologyProvider, LookupResult,
    SubsumptionResult, TerminologyProvider, TranslationResult, ValidationResult, ValueSetConcept,
    ValueSetExpansion,
};
use octofhir_fhir_model::{CachedTerminologyProvider, TerminologyCacheConfig};
use serde::{Deserialize, Serialize};
use sqlx_postgres::{PgPool, PgPoolCopyExt};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Default cache TTL: 1 hour
const DEFAULT_CACHE_TTL_SECS: u64 = 3600;

/// Default terminology server URL
const DEFAULT_TERMINOLOGY_SERVER: &str = "https://tx.fhir.org/r4";

/// Threshold for large ValueSet expansion (500 codes)
/// Below this: Use traditional IN clause
/// At or above: Use temporary table with bulk insert
const LARGE_EXPANSION_THRESHOLD: usize = 500;

/// Result of ValueSet expansion for search operations.
///
/// Determines the optimal strategy based on expansion size:
/// - Small (<500 codes): Use IN clause with parameterized query
/// - Large (≥500 codes): Use temporary table with session ID
#[derive(Debug, Clone)]
pub enum ExpansionResult {
    /// Small expansion: Use traditional IN clause
    /// Contains the list of codes to include in the query
    InClause(Vec<ValueSetConcept>),

    /// Large expansion: Use temporary table
    /// Contains the session ID for the temp table lookup
    TempTable(String),
}

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

/// Configuration for terminology service integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminologyConfig {
    /// Enable terminology service integration
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// URL of the terminology server (default: https://tx.fhir.org/r4)
    #[serde(default = "default_server_url")]
    pub server_url: String,

    /// Cache TTL in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_server_url() -> String {
    DEFAULT_TERMINOLOGY_SERVER.to_string()
}

fn default_cache_ttl() -> u64 {
    DEFAULT_CACHE_TTL_SECS
}

impl Default for TerminologyConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            server_url: default_server_url(),
            cache_ttl_secs: default_cache_ttl(),
        }
    }
}

/// Errors from terminology operations.
#[derive(Debug, Error)]
pub enum TerminologyError {
    #[error("Terminology service is disabled")]
    Disabled,

    #[error("Failed to create HTTP client: {0}")]
    HttpClientError(String),

    #[error("Remote terminology error: {0}")]
    RemoteError(String),

    #[error("ValueSet not found: {0}")]
    ValueSetNotFound(String),

    #[error("CodeSystem not found: {0}")]
    CodeSystemNotFound(String),
}


/// Hybrid terminology provider with two-tier lookup.
///
/// Priority order:
/// 1. Local FHIR packages via CanonicalManager (fast, no network)
/// 2. Remote terminology server with moka caching (CachedTerminologyProvider)
///
/// The caching is handled by `CachedTerminologyProvider<HttpTerminologyProvider>`
/// from the shared `octofhir-fhir-model` crate, which uses moka for TTL-based caching.
#[derive(Debug)]
pub struct HybridTerminologyProvider {
    /// Reference to the canonical manager for local package lookups
    canonical_manager: Arc<CanonicalManager>,

    /// Remote terminology provider with caching (using shared infrastructure)
    remote: CachedTerminologyProvider<HttpTerminologyProvider>,
}

impl HybridTerminologyProvider {
    /// Create a new hybrid terminology provider.
    ///
    /// # Arguments
    ///
    /// * `canonical_manager` - Reference to the canonical manager for local lookups
    /// * `config` - Terminology configuration
    ///
    /// # Returns
    ///
    /// A new `HybridTerminologyProvider` or an error if HTTP client creation fails.
    pub fn new(
        canonical_manager: Arc<CanonicalManager>,
        config: &TerminologyConfig,
    ) -> Result<Self, TerminologyError> {
        let http_provider = HttpTerminologyProvider::new(config.server_url.clone())
            .map_err(|e| TerminologyError::HttpClientError(e.to_string()))?;

        let cache_config = TerminologyCacheConfig::default()
            .with_validation_ttl(Duration::from_secs(config.cache_ttl_secs))
            .with_expansion_ttl(Duration::from_secs(config.cache_ttl_secs));

        let remote = CachedTerminologyProvider::new(http_provider, cache_config);

        Ok(Self {
            canonical_manager,
            remote,
        })
    }

    /// Clear all caches.
    pub fn clear_cache(&self) {
        self.remote.clear_cache();
        tracing::debug!("Cleared terminology caches");
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> CacheStats {
        let stats = self.remote.cache_stats();
        CacheStats {
            expansion_cache_size: stats.expansion_entries as usize,
            validation_cache_size: stats.validation_entries as usize,
        }
    }

    /// Sync pending cache operations (moka is eventually consistent).
    pub async fn sync_cache(&self) {
        self.remote.sync().await;
    }

    /// Try to expand a ValueSet from local packages.
    async fn expand_from_local(&self, valueset_url: &str) -> Option<ValueSetExpansion> {
        // Try to resolve the ValueSet by canonical URL
        let resolved = self.canonical_manager.resolve(valueset_url).await.ok()?;

        // Verify it's a ValueSet
        if resolved.resource.resource_type != "ValueSet" {
            return None;
        }

        let valueset = &resolved.resource.content;

        // Try to get expansion from ValueSet resource
        if let Some(expansion) = valueset.get("expansion") {
            return self.parse_expansion(expansion);
        }

        // If no expansion, try to build from compose
        if let Some(compose) = valueset.get("compose") {
            return self.build_expansion_from_compose(compose).await;
        }

        None
    }

    /// Parse an expansion section from a ValueSet.
    fn parse_expansion(&self, expansion: &serde_json::Value) -> Option<ValueSetExpansion> {
        let contains = expansion
            .get("contains")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(ValueSetConcept {
                            code: item.get("code")?.as_str()?.to_string(),
                            system: item
                                .get("system")
                                .and_then(|s| s.as_str())
                                .map(String::from),
                            display: item
                                .get("display")
                                .and_then(|d| d.as_str())
                                .map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let total = expansion
            .get("total")
            .and_then(|t| t.as_u64())
            .map(|t| t as u32);

        Some(ValueSetExpansion {
            contains,
            total,
            parameters: Vec::new(),
            timestamp: expansion
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(String::from),
        })
    }

    /// Build expansion from compose section (simplified).
    async fn build_expansion_from_compose(
        &self,
        compose: &serde_json::Value,
    ) -> Option<ValueSetExpansion> {
        let mut concepts = Vec::new();

        if let Some(includes) = compose.get("include").and_then(|i| i.as_array()) {
            for include in includes {
                let system = include
                    .get("system")
                    .and_then(|s| s.as_str())
                    .map(String::from);

                // Handle explicit concept list
                if let Some(concept_arr) = include.get("concept").and_then(|c| c.as_array()) {
                    for concept in concept_arr {
                        if let Some(code) = concept.get("code").and_then(|c| c.as_str()) {
                            concepts.push(ValueSetConcept {
                                code: code.to_string(),
                                system: system.clone(),
                                display: concept
                                    .get("display")
                                    .and_then(|d| d.as_str())
                                    .map(String::from),
                            });
                        }
                    }
                }

                // Note: For full expansion, we'd need to handle:
                // - filter criteria
                // - valueSet references
                // - CodeSystem lookups
                // This is a simplified implementation that only handles explicit concepts.
            }
        }

        if concepts.is_empty() {
            return None;
        }

        Some(ValueSetExpansion {
            contains: concepts,
            total: None,
            parameters: Vec::new(),
            timestamp: None,
        })
    }

    /// Try to validate code against local ValueSet.
    async fn validate_code_vs_local(
        &self,
        valueset_url: &str,
        system: Option<&str>,
        code: &str,
    ) -> Option<bool> {
        let expansion = self.expand_from_local(valueset_url).await?;

        // Check if code is in expansion
        let found = expansion.contains.iter().any(|c| {
            if c.code != code {
                return false;
            }
            // If system specified, must match
            if let Some(sys) = system {
                if let Some(ref concept_sys) = c.system {
                    return concept_sys == sys;
                }
                // No system in concept, but system was required
                return false;
            }
            true
        });

        Some(found)
    }

    /// Expand a ValueSet for search operations with optimal strategy selection.
    ///
    /// This method automatically chooses the best expansion strategy based on size:
    /// - Small expansions (<500 codes): Returns InClause for direct SQL IN clause
    /// - Large expansions (≥500 codes): Returns TempTable with session ID after bulk insert
    ///
    /// # Arguments
    ///
    /// * `pool` - PostgreSQL connection pool for temp table operations
    /// * `valueset_url` - Canonical URL of the ValueSet to expand
    /// * `parameters` - Optional expansion parameters (e.g., filter, count, offset)
    ///
    /// # Returns
    ///
    /// `ExpansionResult` indicating which strategy to use:
    /// - `InClause(concepts)`: Use traditional SQL IN clause
    /// - `TempTable(session_id)`: Use JOIN with temp_valueset_codes table
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = provider.expand_valueset_for_search(
    ///     &pool,
    ///     "http://loinc.org/vs/vital-signs",
    ///     None
    /// ).await?;
    ///
    /// match result {
    ///     ExpansionResult::InClause(concepts) => {
    ///         // Use WHERE code IN (...)
    ///     }
    ///     ExpansionResult::TempTable(session_id) => {
    ///         // Use JOIN with temp_valueset_codes WHERE session_id = ...
    ///     }
    /// }
    /// ```
    pub async fn expand_valueset_for_search(
        &self,
        pool: &PgPool,
        valueset_url: &str,
        parameters: Option<&ExpansionParameters>,
    ) -> Result<ExpansionResult, TerminologyError> {
        // First, expand the ValueSet normally
        let expansion = self
            .expand_valueset(valueset_url, parameters)
            .await
            .map_err(|e| TerminologyError::RemoteError(e.to_string()))?;

        let code_count = expansion.contains.len();

        // Small expansion: use IN clause (traditional approach)
        if code_count < LARGE_EXPANSION_THRESHOLD {
            tracing::debug!(
                valueset = %valueset_url,
                codes = code_count,
                "Using IN clause for small ValueSet expansion"
            );
            return Ok(ExpansionResult::InClause(expansion.contains));
        }

        // Large expansion: use temp table
        tracing::info!(
            valueset = %valueset_url,
            codes = code_count,
            "Using temp table for large ValueSet expansion"
        );

        let session_id = uuid::Uuid::new_v4().to_string();

        // Bulk insert using COPY for maximum performance
        let mut copy_writer = pool
            .copy_in_raw(
                "COPY temp_valueset_codes (session_id, code, system, display) FROM STDIN WITH (FORMAT CSV, DELIMITER E'\\t')"
            )
            .await
            .map_err(|e| {
                TerminologyError::RemoteError(format!("Failed to start COPY operation: {}", e))
            })?;

        // Write all concepts as TSV (Tab-Separated Values)
        for concept in &expansion.contains {
            let line = format!(
                "{}\t{}\t{}\t{}\n",
                session_id,
                concept.code,
                concept.system.as_deref().unwrap_or(""),
                concept.display.as_deref().unwrap_or("")
            );
            copy_writer.send(line.as_bytes()).await.map_err(|e| {
                TerminologyError::RemoteError(format!("Failed to write to COPY: {}", e))
            })?;
        }

        copy_writer.finish().await.map_err(|e| {
            TerminologyError::RemoteError(format!("Failed to finish COPY operation: {}", e))
        })?;

        tracing::debug!(
            session_id = %session_id,
            codes = code_count,
            "Bulk inserted {} codes into temp table",
            code_count
        );

        Ok(ExpansionResult::TempTable(session_id))
    }

    /// Expand a code hierarchy for subsumption searches (`:below` and `:above` modifiers).
    ///
    /// This method finds all codes in a hierarchy relationship with the given code:
    /// - `Below`: Find all descendants (codes subsumed by the given code)
    /// - `Above`: Find all ancestors (codes that subsume the given code)
    ///
    /// # Arguments
    ///
    /// * `system` - The code system URL (required)
    /// * `code` - The code to expand hierarchy for
    /// * `direction` - Whether to find descendants (Below) or ancestors (Above)
    ///
    /// # Returns
    ///
    /// A vector of codes in the hierarchy. If hierarchy expansion is not supported,
    /// returns just the original code as a fallback.
    ///
    /// # SNOMED CT Support
    ///
    /// For SNOMED CT, this uses Expression Constraint Language (ECL):
    /// - `<< code` - Self and descendants (Below)
    /// - `>> code` - Self and ancestors (Above)
    ///
    /// # Performance
    ///
    /// Target: <200ms for most hierarchies
    pub async fn expand_hierarchy(
        &self,
        system: &str,
        code: &str,
        direction: HierarchyDirection,
    ) -> Result<Vec<String>, TerminologyError> {
        tracing::debug!(
            system = system,
            code = code,
            direction = ?direction,
            "Expanding code hierarchy"
        );

        // Special handling for SNOMED CT using ECL
        if system.contains("snomed.info/sct") {
            return self.expand_snomed_hierarchy(system, code, direction).await;
        }

        // For other systems, use remote terminology server
        self.expand_remote_hierarchy(system, code, direction).await
    }

    /// Expand SNOMED CT hierarchy using Expression Constraint Language (ECL).
    ///
    /// SNOMED CT supports hierarchical queries through ECL:
    /// - `<< CODE` - Self and all descendants
    /// - `>> CODE` - Self and all ancestors
    async fn expand_snomed_hierarchy(
        &self,
        system: &str,
        code: &str,
        direction: HierarchyDirection,
    ) -> Result<Vec<String>, TerminologyError> {
        // Build ECL expression
        let ecl = match direction {
            HierarchyDirection::Below => format!("<< {}", code),
            HierarchyDirection::Above => format!(">> {}", code),
        };

        tracing::debug!(
            system = system,
            code = code,
            ecl = %ecl,
            "Using ECL for SNOMED CT hierarchy"
        );

        // Create an implicit ValueSet using ECL
        // Format: system?fhir_vs=ecl/ENCODED_ECL
        let ecl_encoded = urlencoding::encode(&ecl);
        let implicit_vs_url = format!("{}?fhir_vs=ecl/{}", system, ecl_encoded);

        // Expand the implicit ValueSet using inner provider to avoid double-caching
        let expansion = self
            .remote
            .inner()
            .expand_valueset(&implicit_vs_url, None)
            .await
            .map_err(|e| {
                TerminologyError::RemoteError(format!(
                    "Failed to expand SNOMED CT hierarchy: {}",
                    e
                ))
            })?;

        // Extract codes from expansion
        let codes: Vec<String> = expansion.contains.iter().map(|c| c.code.clone()).collect();

        tracing::debug!(
            system = system,
            code = code,
            hierarchy_size = codes.len(),
            "Expanded SNOMED CT hierarchy"
        );

        Ok(codes)
    }

    /// Expand hierarchy using remote terminology server.
    ///
    /// This is a generic implementation that attempts to use the $subsumes operation
    /// or falls back to returning just the code.
    async fn expand_remote_hierarchy(
        &self,
        system: &str,
        code: &str,
        direction: HierarchyDirection,
    ) -> Result<Vec<String>, TerminologyError> {
        // Note: Full implementation would require:
        // 1. $subsumes operation to check parent/child relationships
        // 2. Recursive traversal to build complete hierarchy
        // 3. Or use of system-specific hierarchy extensions
        //
        // For now, we implement a simplified version that returns the code itself
        // and logs a warning about limited support.

        tracing::warn!(
            system = system,
            code = code,
            direction = ?direction,
            "Generic hierarchy expansion not fully implemented, returning code only"
        );

        // Future enhancement: Could try to use $subsumes operation
        // See: https://www.hl7.org/fhir/codesystem-operation-subsumes.html

        Ok(vec![code.to_string()])
    }
}

#[async_trait]
impl TerminologyProvider for HybridTerminologyProvider {
    async fn validate_code(
        &self,
        code: &str,
        system: &str,
        version: Option<&str>,
    ) -> octofhir_fhir_model::error::Result<bool> {
        // Delegate to remote (cached provider handles caching)
        self.remote.validate_code(code, system, version).await
    }

    async fn expand_valueset(
        &self,
        valueset_url: &str,
        parameters: Option<&ExpansionParameters>,
    ) -> octofhir_fhir_model::error::Result<ValueSetExpansion> {
        // 1. Try local packages first (fast, no network)
        if let Some(expansion) = self.expand_from_local(valueset_url).await {
            tracing::debug!(valueset = %valueset_url, "Found ValueSet in local packages");
            return Ok(expansion);
        }

        // 2. Fall back to cached remote (CachedTerminologyProvider handles caching)
        tracing::debug!(valueset = %valueset_url, "Falling back to remote terminology server");
        self.remote.expand_valueset(valueset_url, parameters).await
    }

    async fn translate_code(
        &self,
        source_code: &str,
        target_system: &str,
        concept_map_url: Option<&str>,
    ) -> octofhir_fhir_model::error::Result<TranslationResult> {
        // Delegate to remote (translation is not cached)
        self.remote
            .translate_code(source_code, target_system, concept_map_url)
            .await
    }

    async fn lookup_code(
        &self,
        system: &str,
        code: &str,
        version: Option<&str>,
        properties: Option<Vec<&str>>,
    ) -> octofhir_fhir_model::error::Result<LookupResult> {
        // Delegate to remote (cached provider handles caching)
        self.remote
            .lookup_code(system, code, version, properties)
            .await
    }

    async fn validate_code_vs(
        &self,
        valueset: &str,
        system: Option<&str>,
        code: &str,
        display: Option<&str>,
    ) -> octofhir_fhir_model::error::Result<ValidationResult> {
        // 1. Try local validation first
        if let Some(result) = self.validate_code_vs_local(valueset, system, code).await {
            tracing::debug!(
                valueset = %valueset,
                code = %code,
                result = result,
                "Validated code against local ValueSet"
            );
            return Ok(ValidationResult {
                result,
                display: None,
                message: None,
            });
        }

        // 2. Fall back to cached remote (CachedTerminologyProvider handles caching)
        self.remote
            .validate_code_vs(valueset, system, code, display)
            .await
    }

    async fn subsumes(
        &self,
        system: &str,
        parent: &str,
        child: &str,
    ) -> octofhir_fhir_model::error::Result<SubsumptionResult> {
        // Delegate to remote
        self.remote.subsumes(system, parent, child).await
    }

    async fn test_connection(&self) -> octofhir_fhir_model::error::Result<ConnectionStatus> {
        // Delegate to remote
        self.remote.test_connection().await
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub expansion_cache_size: usize,
    pub validation_cache_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminology_config_defaults() {
        let config = TerminologyConfig::default();
        assert!(config.enabled);
        assert_eq!(config.server_url, "https://tx.fhir.org/r4");
        assert_eq!(config.cache_ttl_secs, 3600);
    }
}
