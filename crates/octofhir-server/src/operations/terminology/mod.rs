//! Terminology Operations Module
//!
//! This module provides FHIR terminology service operations:
//! - `$expand` - Expand a ValueSet to its enumerated codes
//! - `$validate-code` - Validate a code against a CodeSystem or ValueSet
//! - `$lookup` - Look up a code in a CodeSystem
//! - `$subsumes` - Test subsumption relationship between codes
//! - `$translate` - Translate a code using ConceptMap
//! - `$closure` - Maintain a transitive closure table for concept hierarchies
//!
//! ## Caching
//!
//! The `cache` module provides an LRU cache for CodeSystem and ConceptMap
//! resources to avoid repeated canonical manager lookups.

pub mod cache;
pub mod closure;
pub mod expand;
pub mod lookup;
pub mod subsumes;
pub mod translate;
pub mod validate_code;

pub use cache::{TerminologyResourceCache, get_cache};
pub use closure::ClosureOperation;
pub use expand::ExpandOperation;
pub use lookup::LookupOperation;
pub use subsumes::SubsumesOperation;
pub use translate::TranslateOperation;
pub use validate_code::ValidateCodeOperation;

use serde_json::Value;
use std::borrow::Cow;

/// Types a query-string value is also offered under; the handlers each read one of them.
const STRING_ALIASES: [&str; 5] = [
    "valueUri",
    "valueCode",
    "valueCanonical",
    "valueUrl",
    "valueId",
];

/// GET requests reach the handlers as Parameters whose values are all `valueString`: the query
/// string carries no types. The handlers read typed values (`url` as `valueUri`, `count` as
/// `valueInteger`, …), so a parameter that only has `valueString` is also given those types.
/// Parameters that already carry a typed value (a POSTed Parameters resource) are left alone.
pub(crate) fn lenient_parameters(params: &Value) -> Cow<'_, Value> {
    let untyped = |parameter: &Value| {
        parameter.as_object().is_some_and(|object| {
            object.contains_key("valueString")
                && object.keys().filter(|key| key.starts_with("value")).count() == 1
        })
    };
    let Some(parameters) = params.get("parameter").and_then(Value::as_array) else {
        return Cow::Borrowed(params);
    };
    if !parameters.iter().any(untyped) {
        return Cow::Borrowed(params);
    }

    let mut normalized = params.clone();
    let Some(parameters) = normalized
        .get_mut("parameter")
        .and_then(Value::as_array_mut)
    else {
        return Cow::Borrowed(params);
    };
    for parameter in parameters.iter_mut().filter(|parameter| untyped(parameter)) {
        let Some(object) = parameter.as_object_mut() else {
            continue;
        };
        let Some(text) = object
            .get("valueString")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        for alias in STRING_ALIASES {
            object.insert(alias.to_owned(), Value::String(text.clone()));
        }
        if let Ok(number) = text.parse::<i64>() {
            object.insert("valueInteger".to_owned(), Value::from(number));
        }
        if let Ok(flag) = text.parse::<bool>() {
            object.insert("valueBoolean".to_owned(), Value::Bool(flag));
        }
    }
    Cow::Owned(normalized)
}

#[cfg(test)]
mod tests {
    use super::lenient_parameters;
    use serde_json::json;

    #[test]
    fn query_string_values_are_offered_under_typed_names() {
        let get = json!({"resourceType": "Parameters", "parameter": [
            {"name": "url", "valueString": "http://hl7.org/fhir/ValueSet/x"},
            {"name": "count", "valueString": "5"},
            {"name": "activeOnly", "valueString": "false"}
        ]});
        let normalized = lenient_parameters(&get);
        let parameters = normalized["parameter"].as_array().unwrap();
        assert_eq!(parameters[0]["valueUri"], "http://hl7.org/fhir/ValueSet/x");
        assert_eq!(parameters[1]["valueInteger"], 5);
        assert_eq!(parameters[2]["valueBoolean"], false);
    }

    #[test]
    fn typed_post_parameters_are_untouched() {
        let post = json!({"resourceType": "Parameters", "parameter": [
            {"name": "url", "valueUri": "http://hl7.org/fhir/ValueSet/x"},
            {"name": "count", "valueInteger": 5}
        ]});
        assert!(matches!(
            lenient_parameters(&post),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
