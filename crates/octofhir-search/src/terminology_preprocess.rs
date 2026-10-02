//! Pre-expand FHIR terminology modifiers into Token values and trusted predicates.
//!
//! Per FHIR R4 §3.1.1.5.5 (https://hl7.org/fhir/R4/search.html#in) and
//! §3.1.1.5.11 (https://hl7.org/fhir/R4/search.html#token) the `:in` and
//! `:not-in` modifiers on a Token search parameter take a ValueSet URL and
//! match against any code in the expanded set.
//!
//! Because the SQL builder pipeline (`dispatch_search`) is synchronous and
//! `TerminologyProvider::expand_valueset` is async, this module runs ahead of
//! query building to expand the ValueSet, then rewrites the query parameters
//! and constraints before SQL compilation:
//!
//! - `code:in=<vsUrl>`     →  `code=<sys>|<code>,<sys>|<code>,…`
//! - `code:not-in=<vsUrl>` → repeated `code:not` predicates, one per code.
//!
//! Spec equivalence:
//! - `:in` matches the SP when the indexed code is in the expanded VS — the
//!   rewritten Token OR matches exactly the same set.
//! - `:not-in` negates membership in the entire expanded set. Comma-separated
//!   ValueSet alternatives are OR branches; repeated occurrences are AND.
//! - Empty sets produce explicit false (`:in`) or true (`:not-in`) predicates.
//!   Expanded token literals retain FHIR escaping for separator characters.
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
//! - `:above` / `:below` use the concrete `HybridTerminologyProvider` through
//!   [`pre_expand_subsumption_modifiers`].

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

/// Pre-expand `:in` and `:not-in` Token modifiers in-place on `params`.
///
/// Walks each `(key, [value_entry,…])` pair in `params.parameters`. For Token
/// search parameters with a `:in` or `:not-in` modifier, the value entries
/// (each of which may carry comma-separated ValueSet URLs for OR-of-ValueSets)
/// become escaped `system|code` literals for `:in`, or Boolean constraints for
/// `:not-in` and empty expansions. Existing trusted constraints are preserved.
pub async fn pre_expand_terminology_modifiers(
    params: &mut SearchParams,
    registry: &SearchParameterRegistry,
    resource_type: &str,
    terminology: &Arc<dyn TerminologyProvider>,
    max_expansion: usize,
) -> Result<(), TerminologyPreprocessError> {
    use octofhir_storage::SearchConstraint;
    let mut rewrites = Vec::new();
    let mut predicates = Vec::new();
    for (key, value_entries) in &params.parameters {
        let Some((name, modifier)) = key.split_once(':') else {
            continue;
        };
        if !matches!(modifier, "in" | "not-in") {
            continue;
        }
        let Some(definition) = registry.get(resource_type, name) else {
            continue;
        };
        if definition.param_type != SearchParameterType::Token {
            continue;
        }
        if value_entries.is_empty() {
            return Err(TerminologyPreprocessError::ExpansionFailed {
                vs: key.clone(),
                message: "ValueSet canonical must not be empty".into(),
            });
        }
        let mut values = Vec::new();
        for value_entry in value_entries {
            let mut codes = Vec::new();
            let mut alternatives = Vec::new();
            let mut expanded = 0usize;
            for raw_url in crate::parser::split_escaped(value_entry, ',') {
                let url = crate::parser::unescape_search_value(raw_url).map_err(|error| {
                    TerminologyPreprocessError::ExpansionFailed {
                        vs: value_entry.clone(),
                        message: error.to_string(),
                    }
                })?;
                if url.trim().is_empty() {
                    return Err(TerminologyPreprocessError::ExpansionFailed {
                        vs: value_entry.clone(),
                        message: "ValueSet canonical must not be empty".into(),
                    });
                }
                let expansion = terminology
                    .expand_valueset(&url, None)
                    .await
                    .map_err(|error| TerminologyPreprocessError::ExpansionFailed {
                        vs: url.clone(),
                        message: error.to_string(),
                    })?;
                expanded = expanded.saturating_add(expansion.contains.len());
                if expanded > max_expansion {
                    return Err(TerminologyPreprocessError::ExpansionTooLarge {
                        vs: value_entry.clone(),
                        actual: expanded,
                        limit: max_expansion,
                    });
                }
                let mut set_codes: Vec<_> = expansion
                    .contains
                    .into_iter()
                    .filter(|concept| !concept.code.is_empty())
                    .map(|concept| {
                        let code = escape_token_literal(&concept.code);
                        match concept.system.filter(|system| !system.is_empty()) {
                            Some(system) => format!("{}|{code}", escape_token_literal(&system)),
                            None => code,
                        }
                    })
                    .collect();
                set_codes.sort();
                set_codes.dedup();
                if modifier == "not-in" {
                    // Negate the entire set, then OR the ValueSet alternatives.
                    // Repeated :not occurrences are AND, one per expanded code.
                    alternatives.push(if set_codes.is_empty() {
                        SearchConstraint::And(Vec::new())
                    } else {
                        SearchConstraint::Query(std::collections::HashMap::from([(
                            format!("{name}:not"),
                            set_codes,
                        )]))
                    });
                } else {
                    codes.extend(set_codes);
                }
            }
            if modifier == "not-in" {
                predicates.push(SearchConstraint::Or(alternatives));
            } else {
                codes.sort();
                codes.dedup();
                if codes.is_empty() {
                    // FHIR codes are arbitrary strings; a sentinel can collide.
                    predicates.push(SearchConstraint::Or(Vec::new()));
                } else {
                    values.push(codes.join(","));
                }
            }
        }
        rewrites.push((key.clone(), name.to_string(), values));
    }
    for (old, name, values) in rewrites {
        params.parameters.remove(&old);
        if !values.is_empty() {
            params.parameters.entry(name).or_default().extend(values);
        }
    }
    if !predicates.is_empty() {
        if let Some(existing) = params.constraint.take() {
            predicates.push(existing);
        }
        params.constraint = Some(SearchConstraint::And(predicates));
    }
    Ok(())
}

fn escape_token_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace('|', "\\|")
        .replace('$', "\\$")
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

        if value_entries.is_empty() {
            return Err(TerminologyPreprocessError::ExpansionFailed {
                vs: key.clone(),
                message: "Hierarchy criterion must not be empty".into(),
            });
        }

        let mut new_value_entries: Vec<String> = Vec::with_capacity(value_entries.len());
        for value_entry in value_entries {
            let mut tokens: Vec<String> = Vec::new();
            for sys_code in crate::parser::split_escaped(value_entry, ',') {
                let (system, code) = parse_hierarchy_seed(sys_code)?;

                let hierarchy_codes = terminology
                    .expand_hierarchy(&system, &code, direction)
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
                    tokens.push(format!(
                        "{}|{}",
                        escape_token_literal(&system),
                        escape_token_literal(&c)
                    ));
                }
                // Always include the seed code itself — the hierarchy result
                // may or may not include it depending on the system.
                tokens.push(format!(
                    "{}|{}",
                    escape_token_literal(&system),
                    escape_token_literal(&code)
                ));
                tokens.sort();
                tokens.dedup();
                if tokens.len() > max_expansion {
                    return Err(TerminologyPreprocessError::ExpansionTooLarge {
                        vs: value_entry.clone(),
                        actual: tokens.len(),
                        limit: max_expansion,
                    });
                }
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

fn parse_hierarchy_seed(value: &str) -> Result<(String, String), TerminologyPreprocessError> {
    let parts = crate::parser::split_escaped(value, '|')
        .map(crate::parser::unescape_search_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| TerminologyPreprocessError::ExpansionFailed {
            vs: value.into(),
            message: error.to_string(),
        })?;
    match parts.as_slice() {
        [system, code] if !system.trim().is_empty() && !code.trim().is_empty() => {
            Ok((system.clone(), code.clone()))
        }
        _ => Err(TerminologyPreprocessError::ExpansionFailed {
            vs: value.into(),
            message: "Hierarchy modifier requires a nonempty `system|code` value".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_literals_keep_escaped_separators_and_reject_invalid_criteria() {
        assert_eq!(
            parse_hierarchy_seed(r"urn:example\|edition|a\,b\|c\$d\\e").unwrap(),
            ("urn:example|edition".into(), "a,b|c$d\\e".into())
        );
        for invalid in [
            "",
            "|code",
            "system|",
            "system|a|b",
            r"system|a\q",
            "system| ",
        ] {
            assert!(parse_hierarchy_seed(invalid).is_err(), "{invalid}");
        }
    }
}
