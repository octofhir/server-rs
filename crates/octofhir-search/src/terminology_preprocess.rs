//! Pre-expand FHIR terminology modifiers into plain Token search values.
//!
//! Per FHIR R4 §3.1.1.5.5 (https://hl7.org/fhir/R4/search.html#in) and
//! §3.1.1.5.11 (https://hl7.org/fhir/R4/search.html#token) the `:in` and
//! `:not-in` modifiers on a Token search parameter take a ValueSet URL and
//! match against any code in the expanded set.
//!
//! Because the SQL builder pipeline (`dispatch_search`) is synchronous and
//! `TerminologyProvider::expand_valueset` is async, this module runs ahead of
//! query building to expand the ValueSet, then rewrites the query parameters
//! so `dispatch_search` sees an ordinary Token comma-OR query:
//!
//! - `code:in=<vsUrl>`     →  `code=<sys>|<code>,<sys>|<code>,…`
//! - `code:not-in=<vsUrl>` →  `code:not=<sys>|<code>,<sys>|<code>,…`
//!
//! Spec equivalence:
//! - `:in` matches the SP when the indexed code is in the expanded VS — the
//!   rewritten Token OR matches exactly the same set.
//! - `:not-in` is the inverse — `:not=A,B,…` matches when the SP does not
//!   equal any of A, B, … (per §3.1.1.5.5 the `:not` modifier semantics).
//!
//! Limitations:
//! - Inline expansion only. Each expanded code becomes an OR branch in the
//!   rewritten query, so the expansion is capped (default
//!   [`DEFAULT_MAX_EXPANSION_SIZE`], override via
//!   `OCTOFHIR__SEARCH__MAX_VALUESET_EXPANSION`); a ValueSet larger than the cap
//!   is rejected with `ExpansionTooLarge` rather than generating a query that
//!   hangs Postgres.
//! - Only top-level params are walked. Terminology modifiers nested inside
//!   `_has:…` or chained-reference parameters reach the sync dispatcher
//!   directly and still return `NotImplemented`.
//! - `:above` / `:below` (subsumption) need `HybridTerminologyProvider`'s
//!   `expand_hierarchy`, which is not exposed on the `TerminologyProvider`
//!   trait. Those modifiers continue to return `NotImplemented` until the
//!   trait is extended or a concrete-typed plumbing is added.

use crate::parameters::SearchParameterType;
use crate::registry::SearchParameterRegistry;
use crate::terminology::{HierarchyDirection, HybridTerminologyProvider};
use octofhir_fhir_model::terminology::TerminologyProvider;
use octofhir_storage::SearchParams;
use std::sync::Arc;

/// Default ceiling on expanded concepts before pre-expansion gives up. The
/// inline rewrite turns each expanded code into an OR branch, so an unbounded
/// expansion would generate a pathologically large query that hangs Postgres —
/// the cap keeps `:in`/`:not-in`/`:above`/`:below` fast. Overridable per
/// deployment via `OCTOFHIR__SEARCH__MAX_VALUESET_EXPANSION`.
pub const DEFAULT_MAX_EXPANSION_SIZE: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum TerminologyPreprocessError {
    #[error("ValueSet '{vs}' expanded to {actual} codes — exceeds configured limit of {limit}")]
    ExpansionTooLarge {
        vs: String,
        actual: usize,
        limit: usize,
    },

    #[error("Failed to expand ValueSet '{vs}': {message}")]
    ExpansionFailed { vs: String, message: String },
}

/// Sentinel value emitted for `:in` against an empty ValueSet expansion. The
/// downstream Token search never matches against this string, so the search
/// correctly returns no results (spec: `:in` against empty VS matches nothing).
const NO_MATCH_SENTINEL: &str = "__fhir262_no_match__";

/// Pre-expand `:in` and `:not-in` Token modifiers in-place on `params`.
///
/// Walks each `(key, [value_entry,…])` pair in `params.parameters`. For Token
/// search parameters with a `:in` or `:not-in` modifier, the value entries
/// (each of which may carry comma-separated ValueSet URLs for OR-of-ValueSets)
/// are replaced by their expansion as `system|code` token literals.
pub async fn pre_expand_terminology_modifiers(
    params: &mut SearchParams,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    terminology: &Arc<dyn TerminologyProvider>,
    max_expansion: usize,
) -> Result<(), TerminologyPreprocessError> {
    let mut rewrites: Vec<(String, String, Vec<String>)> = Vec::new();

    for (key, value_entries) in &params.parameters {
        let Some((name, modifier)) = key.split_once(':') else {
            continue;
        };
        let is_in = modifier == "in";
        let is_not_in = modifier == "not-in";
        if !(is_in || is_not_in) {
            continue;
        }

        let Some(def) = registry.get(resource_type, name) else {
            continue;
        };
        if def.param_type != SearchParameterType::Token {
            continue;
        }

        let mut new_value_entries: Vec<String> = Vec::with_capacity(value_entries.len());
        for value_entry in value_entries {
            let mut codes: Vec<String> = Vec::new();
            for vs_url in value_entry
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let expansion = terminology
                    .expand_valueset(vs_url, None)
                    .await
                    .map_err(|e| TerminologyPreprocessError::ExpansionFailed {
                        vs: vs_url.to_string(),
                        message: e.to_string(),
                    })?;

                if expansion.contains.len() > max_expansion {
                    return Err(TerminologyPreprocessError::ExpansionTooLarge {
                        vs: vs_url.to_string(),
                        actual: expansion.contains.len(),
                        limit: max_expansion,
                    });
                }

                for concept in expansion.contains {
                    if concept.code.is_empty() {
                        continue;
                    }
                    match concept.system.as_deref() {
                        Some(sys) if !sys.is_empty() => {
                            codes.push(format!("{sys}|{}", concept.code));
                        }
                        _ => codes.push(concept.code),
                    }
                }
            }

            codes.sort();
            codes.dedup();

            if codes.is_empty() {
                // Empty expansion semantics (§3.1.1.5.5):
                //   :in     → matches nothing
                //   :not-in → matches everything
                // For :in, plant a sentinel that won't match any indexed code,
                // forcing an empty result. For :not-in, drop this occurrence
                // (no constraint added) — the AND with other params still
                // restricts correctly.
                if is_in {
                    new_value_entries.push(NO_MATCH_SENTINEL.to_string());
                }
                continue;
            }
            new_value_entries.push(codes.join(","));
        }

        let new_key = if is_not_in {
            format!("{name}:not")
        } else {
            name.to_string()
        };
        rewrites.push((key.clone(), new_key, new_value_entries));
    }

    for (old_key, new_key, new_values) in rewrites {
        params.parameters.remove(&old_key);
        if new_values.is_empty() {
            continue;
        }
        let entry = params.parameters.entry(new_key).or_default();
        entry.extend(new_values);
    }

    Ok(())
}

/// Pre-expand `:above` and `:below` Token subsumption modifiers in-place.
///
/// Per FHIR R4 §3.1.1.5 "modifiers" + §3.1.1.5.11 (search.html#token):
/// - `:below=system|code` matches the SP when the resource code is `code`
///   or any descendant in the named code system's hierarchy.
/// - `:above=system|code` matches when the resource code is `code` or any
///   ancestor.
///
/// The hierarchy traversal lives on `HybridTerminologyProvider` (SNOMED ECL
/// or remote `$expand` with ECL), so this function takes the concrete
/// provider — not the dyn-trait used elsewhere.
///
/// Rewrites in place:
///   `code:below=sys|c` → `code=sys|c,sys|child1,sys|child2,…`
///   `code:above=sys|c` → `code=sys|c,sys|parent1,…`
///
/// Failure modes mirror [`pre_expand_terminology_modifiers`]: hierarchies
/// larger than [`DEFAULT_MAX_EXPANSION_SIZE`] return `ExpansionTooLarge`.
pub async fn pre_expand_subsumption_modifiers(
    params: &mut SearchParams,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    terminology: &Arc<HybridTerminologyProvider>,
    max_expansion: usize,
) -> Result<(), TerminologyPreprocessError> {
    let mut rewrites: Vec<(String, String, Vec<String>)> = Vec::new();

    for (key, value_entries) in &params.parameters {
        let Some((name, modifier)) = key.split_once(':') else {
            continue;
        };
        let direction = match modifier {
            "below" => HierarchyDirection::Below,
            "above" => HierarchyDirection::Above,
            _ => continue,
        };

        let Some(def) = registry.get(resource_type, name) else {
            continue;
        };
        if def.param_type != SearchParameterType::Token {
            continue;
        }

        let mut new_value_entries: Vec<String> = Vec::with_capacity(value_entries.len());
        for value_entry in value_entries {
            let mut tokens: Vec<String> = Vec::new();
            for sys_code in value_entry
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let (system, code) = match sys_code.split_once('|') {
                    Some((sys, code)) if !sys.is_empty() && !code.is_empty() => (sys, code),
                    _ => {
                        return Err(TerminologyPreprocessError::ExpansionFailed {
                            vs: sys_code.to_string(),
                            message: format!(
                                "{}/{} modifier requires `system|code` value",
                                name,
                                if direction == HierarchyDirection::Below {
                                    "below"
                                } else {
                                    "above"
                                }
                            ),
                        });
                    }
                };

                let hierarchy_codes = terminology
                    .expand_hierarchy(system, code, direction)
                    .await
                    .map_err(|e| TerminologyPreprocessError::ExpansionFailed {
                        vs: sys_code.to_string(),
                        message: e.to_string(),
                    })?;

                if hierarchy_codes.len() > max_expansion {
                    return Err(TerminologyPreprocessError::ExpansionTooLarge {
                        vs: sys_code.to_string(),
                        actual: hierarchy_codes.len(),
                        limit: max_expansion,
                    });
                }

                for c in hierarchy_codes {
                    if c.is_empty() {
                        continue;
                    }
                    tokens.push(format!("{system}|{c}"));
                }
                // Always include the seed code itself — the hierarchy result
                // may or may not include it depending on the system.
                tokens.push(format!("{system}|{code}"));
            }

            tokens.sort();
            tokens.dedup();
            if tokens.is_empty() {
                continue;
            }
            new_value_entries.push(tokens.join(","));
        }

        rewrites.push((key.clone(), name.to_string(), new_value_entries));
    }

    for (old_key, new_key, new_values) in rewrites {
        params.parameters.remove(&old_key);
        if new_values.is_empty() {
            continue;
        }
        let entry = params.parameters.entry(new_key).or_default();
        entry.extend(new_values);
    }

    Ok(())
}
