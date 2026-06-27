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
///
/// Terminology is always enabled - this config controls the server URL and cache settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminologyConfig {
    /// URL of the terminology server (default: https://tx.fhir.org/r4)
    #[serde(default = "default_server_url")]
    pub server_url: String,

    /// Cache TTL in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
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
            server_url: default_server_url(),
            cache_ttl_secs: default_cache_ttl(),
        }
    }
}

/// Errors from terminology operations.
#[derive(Debug, Error)]
pub enum TerminologyError {
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

    /// Process-level cache of ValueSet code-validation results, keyed by
    /// `valueset|system|code`. Binding validation repeats the same
    /// `(valueset, system, code)` lookup many times per resource (e.g. an
    /// ExplanationOfBenefit has 8 currency-bound fields, each re-resolving the
    /// currencies ValueSet + ISO-4217 code system from storage), so memoising the
    /// result collapses that to a single computation.
    vs_validation_cache: dashmap::DashMap<String, ValidationResult>,

    /// Negative cache for remote validation errors, keyed like
    /// `vs_validation_cache`, with the failure instant for TTL expiry. A remote
    /// error (timeout, 4xx on a malformed/unknown ValueSet URL) would otherwise
    /// bypass both caches and re-hit the remote server on every occurrence of
    /// the same code — the dominant write-path stall observed under load.
    vs_validation_negative_cache: dashmap::DashMap<String, (ValidationResult, std::time::Instant)>,

    /// Single-flight guards: at most one in-flight remote validation per cache
    /// key; concurrent duplicates await the winner and read the cache.
    vs_validation_inflight: dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

/// How long a remote validation error stays negative-cached.
const VS_VALIDATION_NEGATIVE_TTL: Duration = Duration::from_secs(60);

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
            vs_validation_cache: dashmap::DashMap::new(),
            vs_validation_negative_cache: dashmap::DashMap::new(),
            vs_validation_inflight: dashmap::DashMap::new(),
        })
    }

    /// Clear all caches.
    pub fn clear_cache(&self) {
        self.remote.clear_cache();
        self.vs_validation_cache.clear();
        self.vs_validation_negative_cache.clear();
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
        // Search/expansion callers only need the codes, not whether the local
        // expansion was complete; an empty expansion is treated as "no local data".
        let (expansion, _complete) = self.expand_from_local_detailed(valueset_url).await?;
        if expansion.contains.is_empty() {
            return None;
        }
        Some(expansion)
    }

    /// Like [`expand_from_local`] but also reports whether the expansion is
    /// authoritative. `complete == false` means some part of the definition could
    /// not be materialized locally, so a "code not in contains" result must be
    /// treated as indeterminate rather than invalid.
    async fn expand_from_local_detailed(
        &self,
        valueset_url: &str,
    ) -> Option<(ValueSetExpansion, bool)> {
        // Try to resolve the ValueSet by canonical URL
        let resolved = self.canonical_manager.resolve(valueset_url).await.ok()?;

        // Verify it's a ValueSet
        if resolved.resource.resource_type != "ValueSet" {
            return None;
        }

        let valueset = &resolved.resource.content;

        // A pre-computed expansion is treated as authoritative.
        if let Some(expansion) = valueset.get("expansion") {
            return self.parse_expansion(expansion).map(|e| (e, true));
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

    /// Build expansion from compose section.
    ///
    /// Returns `(expansion, complete)`. `complete` is `false` when at least one
    /// part of the compose could not be fully materialized locally - a non-enumerable
    /// or unresolved code system, a `filter`, a `valueSet` import, or any `exclude`.
    /// In that case the absence of a code from `contains` is NOT authoritative and
    /// callers must not treat it as a hard "invalid".
    async fn build_expansion_from_compose(
        &self,
        compose: &serde_json::Value,
    ) -> Option<(ValueSetExpansion, bool)> {
        let mut concepts = Vec::new();
        let mut complete = true;

        // We don't apply excludes here, so any exclude makes the result non-authoritative.
        if compose
            .get("exclude")
            .and_then(|e| e.as_array())
            .is_some_and(|a| !a.is_empty())
        {
            complete = false;
        }

        let includes = compose.get("include").and_then(|i| i.as_array());
        if includes.is_none() {
            return None;
        }

        for include in includes.unwrap() {
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
            } else if include.get("filter").is_some() {
                // Filters are not evaluated locally.
                complete = false;
            } else if let Some(ref sys) = system {
                // No explicit concept list - load all codes from the CodeSystem.
                if let Some(cs_concepts) = self.load_codesystem_concepts(sys).await {
                    for concept in cs_concepts {
                        concepts.push(ValueSetConcept {
                            code: concept.code,
                            system: Some(sys.clone()),
                            display: concept.display,
                        });
                    }
                } else {
                    // Code system could not be resolved/enumerated locally
                    // (e.g. urn:iso:std:iso:4217, or any not-present content).
                    complete = false;
                }
            } else {
                // `valueSet` import or an include without system/concept/filter.
                complete = false;
            }
        }

        Some((
            ValueSetExpansion {
                contains: concepts,
                total: None,
                parameters: Vec::new(),
                timestamp: None,
            },
            complete,
        ))
    }

    /// Load all concepts from a CodeSystem by URL.
    async fn load_codesystem_concepts(&self, system_url: &str) -> Option<Vec<ValueSetConcept>> {
        let resolved = self.canonical_manager.resolve(system_url).await.ok()?;

        if resolved.resource.resource_type != "CodeSystem" {
            return None;
        }

        let codesystem = &resolved.resource.content;
        let concept_arr = codesystem.get("concept")?.as_array()?;

        let mut concepts = Vec::new();
        self.collect_concepts_recursive(concept_arr, &mut concepts);
        Some(concepts)
    }

    /// Recursively collect concepts from a CodeSystem hierarchy.
    fn collect_concepts_recursive(
        &self,
        concept_arr: &[serde_json::Value],
        concepts: &mut Vec<ValueSetConcept>,
    ) {
        for concept in concept_arr {
            if let Some(code) = concept.get("code").and_then(|c| c.as_str()) {
                concepts.push(ValueSetConcept {
                    code: code.to_string(),
                    system: None, // System will be set by caller
                    display: concept
                        .get("display")
                        .and_then(|d| d.as_str())
                        .map(String::from),
                });
            }

            // Handle nested concepts
            if let Some(children) = concept.get("concept").and_then(|c| c.as_array()) {
                self.collect_concepts_recursive(children, concepts);
            }
        }
    }

    /// Try to validate code against local ValueSet.
    async fn validate_code_vs_local(
        &self,
        valueset_url: &str,
        system: Option<&str>,
        code: &str,
    ) -> Option<bool> {
        let (expansion, complete) = self.expand_from_local_detailed(valueset_url).await?;

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

        if found {
            return Some(true);
        }

        // Code absent. Only authoritative if the local expansion was complete;
        // otherwise return None so the caller falls back to the remote terminology
        // server instead of falsely rejecting a valid code (e.g. ISO-4217 currency
        // codes whose code system is not enumerable locally).
        if complete { Some(false) } else { None }
    }

    /// Try to validate code against local CodeSystem.
    ///
    /// This checks if the code exists in a locally loaded CodeSystem,
    /// avoiding expensive remote calls to tx.fhir.org.
    async fn validate_code_local(&self, code: &str, system: &str) -> Option<bool> {
        // Try to resolve the CodeSystem by URL
        let resolved = self.canonical_manager.resolve(system).await.ok()?;

        // Verify it's a CodeSystem
        if resolved.resource.resource_type != "CodeSystem" {
            return None;
        }

        let codesystem = &resolved.resource.content;
        let concept_arr = codesystem.get("concept")?.as_array()?;

        // Check recursively if the code exists
        let found = self.find_code_in_concepts(concept_arr, code);
        Some(found)
    }

    /// Recursively search for a code in a concept hierarchy.
    fn find_code_in_concepts(&self, concept_arr: &[serde_json::Value], target_code: &str) -> bool {
        for concept in concept_arr {
            if let Some(code) = concept.get("code").and_then(|c| c.as_str())
                && code == target_code
            {
                return true;
            }

            // Check nested concepts
            if let Some(children) = concept.get("concept").and_then(|c| c.as_array())
                && self.find_code_in_concepts(children, target_code)
            {
                return true;
            }
        }
        false
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
        // 1. Try local validation first (fast, no network)
        // Note: version is ignored for local validation as we use the loaded package version
        if let Some(result) = self.validate_code_local(code, system).await {
            tracing::debug!(
                system = %system,
                code = %code,
                result = result,
                "Validated code against local CodeSystem"
            );
            return Ok(result);
        }

        // 2. Fall back to cached remote (CachedTerminologyProvider handles caching)
        tracing::debug!(
            system = %system,
            code = %code,
            "Falling back to remote terminology server"
        );
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
        // 0. Process-level memoisation. The same (valueset, system, code) is
        // validated repeatedly within and across resources; caching avoids
        // re-resolving the ValueSet/CodeSystem from storage and re-hitting the
        // remote server every time.
        let cache_key = format!("{valueset}|{}|{code}", system.unwrap_or(""));
        if let Some(hit) = self.vs_validation_cache.get(&cache_key) {
            return Ok(hit.clone());
        }
        if let Some(neg) = self.vs_validation_negative_cache.get(&cache_key) {
            if neg.1.elapsed() < VS_VALIDATION_NEGATIVE_TTL {
                return Ok(neg.0.clone());
            }
            drop(neg);
            self.vs_validation_negative_cache.remove(&cache_key);
        }

        // Single-flight: concurrent misses on the same key wait for the first
        // resolver instead of issuing duplicate remote calls.
        let flight = self
            .vs_validation_inflight
            .entry(cache_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = flight.lock().await;

        // Re-check after acquiring the lock — the winner may have populated it.
        if let Some(hit) = self.vs_validation_cache.get(&cache_key) {
            return Ok(hit.clone());
        }
        if let Some(neg) = self.vs_validation_negative_cache.get(&cache_key)
            && neg.1.elapsed() < VS_VALIDATION_NEGATIVE_TTL
        {
            return Ok(neg.0.clone());
        }

        // 1. Try local validation first
        let result = if let Some(result) = self.validate_code_vs_local(valueset, system, code).await
        {
            tracing::debug!(
                valueset = %valueset,
                code = %code,
                result = result,
                "Validated code against local ValueSet"
            );
            ValidationResult {
                result,
                display: None,
                message: None,
            }
        } else {
            // 2. Fall back to cached remote (CachedTerminologyProvider handles caching)
            match self
                .remote
                .validate_code_vs(valueset, system, code, display)
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    // Remote failure (timeout, 4xx on a malformed ValueSet URL,
                    // …). Negative-cache result=false with a short TTL so the
                    // same code does not re-hit the remote on every resource.
                    let negative = ValidationResult {
                        result: false,
                        display: None,
                        message: Some(format!("Terminology server error: {e}")),
                    };
                    self.vs_validation_negative_cache.insert(
                        cache_key.clone(),
                        (negative.clone(), std::time::Instant::now()),
                    );
                    self.vs_validation_inflight.remove(&cache_key);
                    return Ok(negative);
                }
            }
        };

        self.vs_validation_cache
            .insert(cache_key.clone(), result.clone());
        self.vs_validation_inflight.remove(&cache_key);
        Ok(result)
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
        assert_eq!(config.server_url, "https://tx.fhir.org/r4");
        assert_eq!(config.cache_ttl_secs, 3600);
    }
}
