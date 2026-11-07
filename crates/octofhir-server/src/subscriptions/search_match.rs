//! Whether one resource matches search criteria, in memory: the filters of a subscription
//! (`Encounter?patient=Patient/123`, the R4 Backport `backport-filter-criteria`) and the query
//! criteria of a topic (`status=finished`).
//!
//! A key is a filter the topic defines (`canFilterBy`), or a search parameter of the resource
//! type in the server's registry. The parameter's FHIRPath expression selects the values, which
//! compare with the search value by parameter type, as FHIR search does: tokens by
//! `system|code`, references by `Type/id` or id, strings case-insensitively from the start,
//! dates, numbers and quantities with the `eq`/`ne`/`gt`/`lt`/`ge`/`le` prefixes. A value with
//! commas matches any of them.

use std::sync::Arc;

use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine};
use octofhir_search::{ReloadableSearchConfig, SearchParameter, SearchParameterType};
use serde_json::Value;

use super::error::{SubscriptionError, SubscriptionResult};
use super::types::ParsedSubscriptionTopic;

/// Modifiers the matcher implements, by parameter type.
fn supported_modifier(kind: SearchParameterType, modifier: &str) -> bool {
    match kind {
        SearchParameterType::Token => matches!(modifier, "not" | "text"),
        SearchParameterType::String => matches!(modifier, "exact" | "contains"),
        SearchParameterType::Uri => matches!(modifier, "below" | "above"),
        _ => modifier == "missing",
    }
}

/// What a filter key means for a resource type.
#[derive(Debug, Clone)]
pub enum FilterKey {
    /// A search parameter: from the registry, or the one a topic filter's definition names.
    Search(Arc<SearchParameter>),
    /// A topic filter defined by a FHIRPath expression; values compare as text.
    Expression(String),
}

/// Matches resources against search criteria.
pub struct SearchMatcher {
    engine: Arc<FhirPathEngine>,
    search: ReloadableSearchConfig,
}

impl SearchMatcher {
    pub fn new(engine: Arc<FhirPathEngine>, search: ReloadableSearchConfig) -> Self {
        Self { engine, search }
    }

    /// What `key` means for `resource_type` under `topic`: the topic's own filter first, then
    /// the search parameter of that name.
    pub fn resolve(
        &self,
        topic: Option<&ParsedSubscriptionTopic>,
        resource_type: &str,
        key: &str,
    ) -> Option<FilterKey> {
        let registry = self.search.config().registry.clone();
        if let Some(definition) = topic.and_then(|topic| {
            topic.can_filter_by.iter().find(|f| {
                f.filter_parameter == key
                    && f.resource
                        .as_deref()
                        .is_none_or(|r| r.rsplit('/').next() == Some(resource_type))
            })
        }) {
            match definition.filter_definition.as_deref() {
                // R5: a canonical of the SearchParameter that defines the filter.
                Some(url) if url.contains("://") => {
                    return registry.get_by_url(url).map(FilterKey::Search);
                }
                Some(expression) => return Some(FilterKey::Expression(expression.to_owned())),
                None => {}
            }
        }
        registry.get(resource_type, key).map(FilterKey::Search)
    }

    /// Why a filter cannot be applied, if it cannot: an unknown key, or a parameter type or
    /// modifier the matcher does not implement.
    pub fn unsupported(
        &self,
        topic: Option<&ParsedSubscriptionTopic>,
        resource_type: &str,
        key: &str,
        modifier: Option<&str>,
    ) -> Option<String> {
        match self.resolve(topic, resource_type, key) {
            None => Some(format!(
                "`{key}` is neither a filter of the topic nor a search parameter of {resource_type}"
            )),
            Some(FilterKey::Expression(_)) => {
                modifier.map(|m| format!("the topic filter `{key}` takes no modifier (`:{m}`)"))
            }
            Some(FilterKey::Search(param)) => {
                if matches!(
                    param.param_type,
                    SearchParameterType::Composite
                        | SearchParameterType::Special
                        | SearchParameterType::Resource
                ) || param.expression.is_none()
                {
                    return Some(format!(
                        "`{key}` is a {:?} search parameter, which subscription filters do not support",
                        param.param_type
                    ));
                }
                modifier
                    .filter(|m| !supported_modifier(param.param_type, m))
                    .map(|m| format!("the modifier `:{m}` is not supported on `{key}`"))
            }
        }
    }

    /// Whether `resource` matches `key[:modifier]=value`. An unknown key does not match.
    pub async fn matches(
        &self,
        topic: Option<&ParsedSubscriptionTopic>,
        resource: &Value,
        key: &str,
        modifier: Option<&str>,
        value: &str,
    ) -> SubscriptionResult<bool> {
        let resource_type = resource["resourceType"].as_str().unwrap_or_default();
        let Some(key) = self.resolve(topic, resource_type, key) else {
            return Ok(false);
        };
        let (expression, kind, targets) = match &key {
            FilterKey::Expression(expression) => {
                (expression.clone(), SearchParameterType::String, Vec::new())
            }
            FilterKey::Search(param) => {
                let Some(expression) = param.expression.as_deref() else {
                    return Ok(false);
                };
                let (expression, targets) = for_resource_type(expression, resource_type);
                (expression, param.param_type, targets)
            }
        };
        if expression.is_empty() {
            return Ok(false);
        }
        let values = self.values(&expression, resource).await?;
        if modifier == Some("missing") {
            return Ok(values.is_empty() == value.eq_ignore_ascii_case("true"));
        }
        let wanted: Vec<&str> = value.split(',').collect();
        let negate = modifier == Some("not");
        let any = values.iter().any(|actual| {
            wanted.iter().any(|wanted| {
                matches_value(kind, modifier.filter(|_| !negate), actual, wanted, &targets)
            })
        });
        Ok(any != negate)
    }

    /// The values `expression` selects in `resource`, as JSON.
    async fn values(&self, expression: &str, resource: &Value) -> SubscriptionResult<Vec<Value>> {
        let provider = self.engine.get_model_provider();
        let collection = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
            .await
            .map_err(|e| SubscriptionError::FhirPathError(format!("FHIRPath context: {e}")))?;
        let context = EvaluationContext::new(collection, provider, None, None, None);
        let result = self
            .engine
            .evaluate(expression, &context)
            .await
            .map_err(|e| SubscriptionError::FhirPathError(format!("{expression}: {e}")))?;
        Ok(result
            .value
            .into_vec()
            .iter()
            .map(|value| value.to_json_value())
            .collect())
    }
}

/// The parts of a search parameter's expression that apply to `resource_type`, with the
/// reference types `where(resolve() is Type)` asks for (resolving needs the target, which an
/// event does not have: the reference's own type is checked instead).
fn for_resource_type(expression: &str, resource_type: &str) -> (String, Vec<String>) {
    let mut parts = Vec::new();
    let mut targets = Vec::new();
    for part in split_union(expression) {
        let part = part.trim();
        let unwrapped = part.trim_start_matches('(');
        let applies = unwrapped
            .split(['.', ' ', ')'])
            .next()
            .is_some_and(|head| head == resource_type || !head.starts_with(char::is_uppercase));
        if !applies {
            continue;
        }
        let mut part = part.to_owned();
        while let Some(start) = part.find(".where(resolve() is ") {
            let rest = &part[start + ".where(resolve() is ".len()..];
            let Some(end) = rest.find(')') else { break };
            targets.push(rest[..end].trim().to_owned());
            part = format!("{}{}", &part[..start], &rest[end + 1..]);
        }
        parts.push(part);
    }
    (parts.join(" | "), targets)
}

/// Splits `a | b | c` at the top level, not inside parentheses or strings.
fn split_union(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start, mut quoted) = (0i32, 0, false);
    for (index, c) in expression.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            '|' if !quoted && depth == 0 => {
                parts.push(&expression[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&expression[start..]);
    parts
}

fn matches_value(
    kind: SearchParameterType,
    modifier: Option<&str>,
    actual: &Value,
    wanted: &str,
    targets: &[String],
) -> bool {
    match kind {
        SearchParameterType::Token => match modifier {
            Some("text") => text_of(actual)
                .iter()
                .any(|text| text.to_lowercase().contains(&wanted.to_lowercase())),
            _ => token_matches(actual, wanted),
        },
        SearchParameterType::Reference => reference_matches(actual, wanted, targets),
        SearchParameterType::String => {
            let wanted = wanted.to_lowercase();
            text_of(actual).iter().any(|text| match modifier {
                Some("exact") => text == &wanted,
                Some("contains") => text.to_lowercase().contains(&wanted),
                _ => text.to_lowercase().starts_with(&wanted),
            })
        }
        SearchParameterType::Uri => actual.as_str().is_some_and(|uri| match modifier {
            Some("below") => uri.starts_with(wanted),
            Some("above") => wanted.starts_with(uri),
            _ => uri == wanted,
        }),
        SearchParameterType::Date => date_matches(actual, wanted),
        SearchParameterType::Number => {
            let (prefix, number) = prefixed(wanted);
            number_of(actual)
                .zip(number.parse::<f64>().ok())
                .is_some_and(|(a, w)| compare(prefix, a.partial_cmp(&w)))
        }
        SearchParameterType::Quantity => {
            let (prefix, rest) = prefixed(wanted);
            let mut parts = rest.split('|');
            let number = parts.next().and_then(|n| n.parse::<f64>().ok());
            let system = parts.next().filter(|s| !s.is_empty());
            let code = parts.next().filter(|s| !s.is_empty());
            let Some(number) = number else { return false };
            let value = actual["value"].as_f64().or_else(|| actual.as_f64());
            value.is_some_and(|v| compare(prefix, v.partial_cmp(&number)))
                && system.is_none_or(|s| actual["system"].as_str() == Some(s))
                && code.is_none_or(|c| {
                    actual["code"].as_str() == Some(c) || actual["unit"].as_str() == Some(c)
                })
        }
        _ => false,
    }
}

/// `system|code`, `|code` (no system), `system|` (any code), or `code` (any system).
fn token_matches(actual: &Value, wanted: &str) -> bool {
    let (system, code) = match wanted.split_once('|') {
        Some((system, code)) => (Some(system), code),
        None => (None, wanted),
    };
    let pair_matches = |s: Option<&str>, c: Option<&str>| {
        let system_ok = match system {
            None => true,
            Some("") => s.is_none(),
            Some(system) => s == Some(system),
        };
        system_ok && (code.is_empty() || c == Some(code))
    };
    match actual {
        Value::String(text) => system.is_none_or(str::is_empty) && text == code,
        Value::Bool(b) => system.is_none() && b.to_string() == code,
        Value::Object(object) => {
            if let Some(codings) = object.get("coding").and_then(Value::as_array) {
                return codings.iter().any(|coding| token_matches(coding, wanted));
            }
            let code_value = object
                .get("code")
                .or_else(|| object.get("value"))
                .and_then(Value::as_str);
            pair_matches(object.get("system").and_then(Value::as_str), code_value)
        }
        _ => false,
    }
}

/// `Type/id`, an id alone, or an absolute URL. The reference's type must be one the parameter
/// asks for, when it asks.
fn reference_matches(actual: &Value, wanted: &str, targets: &[String]) -> bool {
    let Some(reference) = actual["reference"].as_str().or_else(|| actual.as_str()) else {
        return false;
    };
    let typed = reference
        .rsplit('/')
        .nth(1)
        .is_none_or(|kind| targets.is_empty() || targets.iter().any(|t| t == kind));
    if !typed {
        return false;
    }
    if reference == wanted {
        return true;
    }
    if wanted.contains('/') {
        // A relative wanted value matches the tail of an absolute reference.
        reference.ends_with(&format!("/{wanted}"))
    } else {
        reference.rsplit('/').next() == Some(wanted)
    }
}

/// The text a string parameter searches: the string itself, or the parts of a HumanName or an
/// Address.
fn text_of(actual: &Value) -> Vec<String> {
    match actual {
        Value::String(text) => vec![text.clone()],
        Value::Object(object) => {
            let mut texts = Vec::new();
            for (key, value) in object {
                if key == "extension" || key == "period" || key == "use" {
                    continue;
                }
                match value {
                    Value::String(text) => texts.push(text.clone()),
                    Value::Array(items) => {
                        texts.extend(items.iter().filter_map(Value::as_str).map(str::to_owned))
                    }
                    _ => {}
                }
            }
            texts
        }
        _ => Vec::new(),
    }
}

fn number_of(actual: &Value) -> Option<f64> {
    actual.as_f64().or_else(|| actual["value"].as_f64())
}

/// The comparison prefix of a search value and the rest: `ge2024` is (`ge`, `2024`).
fn prefixed(value: &str) -> (&str, &str) {
    for prefix in ["eq", "ne", "gt", "lt", "ge", "le", "sa", "eb", "ap"] {
        if let Some(rest) = value.strip_prefix(prefix)
            && rest.starts_with(|c: char| c.is_ascii_digit() || c == '-')
        {
            return (prefix, rest);
        }
    }
    ("eq", value)
}

fn compare(prefix: &str, ordering: Option<std::cmp::Ordering>) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match (prefix, ordering) {
        (_, None) => false,
        ("ne", Some(o)) => o != Equal,
        ("gt" | "sa", Some(o)) => o == Greater,
        ("lt" | "eb", Some(o)) => o == Less,
        ("ge", Some(o)) => o != Less,
        ("le", Some(o)) => o != Greater,
        (_, Some(o)) => o == Equal,
    }
}

/// Dates compare at the precision of the search value: `eq2024-05` matches any time in May
/// 2024. A Period matches when it overlaps (`eq`) or by its start and end.
fn date_matches(actual: &Value, wanted: &str) -> bool {
    let (prefix, value) = prefixed(wanted);
    let (start, end) = match actual {
        Value::String(date) => (date.as_str(), date.as_str()),
        Value::Object(period) => (
            period["start"].as_str().unwrap_or(""),
            period["end"].as_str().unwrap_or("\u{10FFFF}"),
        ),
        _ => return false,
    };
    // Cut both sides to the search value's precision, so `2024-05-03T10:00` is `2024-05`
    // against `2024-05`.
    let cut = |date: &str| date.chars().take(value.len()).collect::<String>();
    let (start, end) = (cut(start), cut(end));
    let value = value.to_owned();
    match prefix {
        "ne" => !(start <= value && value <= end),
        "gt" | "sa" => end > value,
        "lt" | "eb" => start < value,
        "ge" => end >= value,
        "le" => start <= value,
        _ => start <= value && value <= end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expressions_keep_the_parts_of_the_resource_type() {
        let (expression, targets) = for_resource_type(
            "AllergyIntolerance.patient | Encounter.subject.where(resolve() is Patient) | (Observation.subject)",
            "Encounter",
        );
        assert_eq!(expression, "Encounter.subject");
        assert_eq!(targets, ["Patient"]);
    }

    #[test]
    fn tokens_references_strings_dates() {
        let coding = json!({"coding": [{"system": "http://loinc.org", "code": "1234-5"}]});
        assert!(token_matches(&coding, "http://loinc.org|1234-5"));
        assert!(token_matches(&coding, "1234-5"));
        assert!(!token_matches(&coding, "|1234-5"));
        assert!(token_matches(&json!("finished"), "finished"));

        let reference = json!({"reference": "Patient/123"});
        assert!(reference_matches(&reference, "Patient/123", &[]));
        assert!(reference_matches(&reference, "123", &["Patient".into()]));
        assert!(!reference_matches(&reference, "123", &["Group".into()]));
        assert!(reference_matches(
            &json!({"reference": "http://x/fhir/Patient/123"}),
            "Patient/123",
            &[]
        ));

        assert!(matches_value(
            SearchParameterType::String,
            None,
            &json!({"family": "Chalmers", "given": ["Peter"]}),
            "chal",
            &[]
        ));
        assert!(date_matches(&json!("2024-05-03T10:00:00Z"), "2024-05"));
        assert!(date_matches(&json!("2024-05-03"), "ge2024-01-01"));
        assert!(!date_matches(&json!("2023-12-31"), "ge2024-01-01"));
        assert!(date_matches(
            &json!({"start": "2024-01-01", "end": "2024-12-31"}),
            "2024-06"
        ));
    }
}
