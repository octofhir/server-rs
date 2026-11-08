//! DICOM matching over transient authorized metadata.
//!
//! # Contents
//! Keyword/tag paths, person-name wildcards, UID alternatives and date/time ranges.
//! # Invariants
//! Patterns are escaped before compiling. Query complexity and regex memory are
//! bounded, and matching never changes the authorized instance set.
use super::{
    attributes::{self, attribute},
    bad,
};
use octofhir_api::ApiError;
use serde_json::Value;
pub(super) struct Filter {
    path: Vec<String>,
    patterns: Vec<regex::Regex>,
    range: Option<(String, String)>,
    temporal: Option<&'static str>,
    numeric: Option<Vec<i64>>,
    names: Vec<PersonPattern>,
}
struct PersonPattern {
    groups: Vec<Vec<Option<regex::Regex>>>,
}
fn pattern(value: &str, insensitive: bool) -> Result<regex::Regex, ApiError> {
    let value = format!(
        "^{}$",
        regex::escape(value)
            .replace("\\*", ".*")
            .replace("\\?", ".")
    );
    regex::RegexBuilder::new(&value)
        .case_insensitive(insensitive)
        .size_limit(65536)
        .build()
        .map_err(|_| bad("Invalid DICOM matching value"))
}
impl PersonPattern {
    fn new(value: &str) -> Result<Self, ApiError> {
        let mut groups = vec![];
        for group in value.split('=') {
            let components = group
                .split('^')
                .map(|part| {
                    if part.trim().is_empty() {
                        Ok(None)
                    } else {
                        pattern(part.trim(), true).map(Some)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            if components.len() > 5 {
                return Err(bad("Invalid DICOM person-name components"));
            }
            groups.push(components);
        }
        if groups.len() > 3 {
            return Err(bad("Invalid DICOM person-name groups"));
        }
        Ok(Self { groups })
    }
    fn matches(&self, value: &Value) -> bool {
        self.groups.iter().enumerate().all(|(group, patterns)| {
            let value = value[["Alphabetic", "Ideographic", "Phonetic"][group]]
                .as_str()
                .unwrap_or("")
                .split('^')
                .map(str::trim)
                .collect::<Vec<_>>();
            patterns.iter().enumerate().all(|(index, pattern)| {
                pattern
                    .as_ref()
                    .is_none_or(|pattern| pattern.is_match(value.get(index).copied().unwrap_or("")))
            })
        })
    }
}
fn bound(value: &str, vr: &str, upper: bool) -> Option<String> {
    if value.is_empty() {
        return Some(String::new());
    }
    if vr == "DA" {
        chrono::NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        return (value.len() == 8).then(|| value.to_owned());
    }
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if !matches!(whole.len(), 2 | 4 | 6)
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (!fraction.is_empty() && whole.len() != 6)
        || value.ends_with('.')
    {
        return None;
    }
    if whole[..2].parse::<u8>().ok()? > 23
        || (whole.len() >= 4 && whole[2..4].parse::<u8>().ok()? > 59)
        || (whole.len() == 6 && whole[4..].parse::<u8>().ok()? > 60)
    {
        return None;
    }
    let fill = if upper { '9' } else { '0' };
    let mut result = whole.to_owned();
    result.extend(std::iter::repeat_n(fill, 6 - whole.len()));
    result.push('.');
    result.push_str(fraction);
    result.extend(std::iter::repeat_n(fill, 6 - fraction.len()));
    Some(result)
}
pub(super) fn strings(value: &Value, path: &[String]) -> Vec<String> {
    let Some(attribute) = value.get(&path[0]) else {
        return vec![];
    };
    let Some(values) = attribute["Value"].as_array() else {
        return vec![];
    };
    if path.len() > 1 {
        return values.iter().flat_map(|v| strings(v, &path[1..])).collect();
    }
    values
        .iter()
        .flat_map(|v| match v {
            Value::String(v) => vec![v.clone()],
            Value::Number(v) => vec![v.to_string()],
            Value::Object(v) if attribute["vr"] == "PN" => v
                .values()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => vec![],
        })
        .collect()
}
impl Filter {
    pub(super) fn new(name: &str, value: &str) -> Result<Self, ApiError> {
        let path =
            attributes::path(name).ok_or_else(|| bad("Unsupported DICOM matching attribute"))?;
        let attr = attribute(&path[0]).expect("known path");
        let vr = if path.len() > 1 { "SH" } else { attr.vr };
        if value.len() > 512 {
            return Err(bad("DICOM matching value is too large"));
        }
        let temporal = matches!(vr, "DA" | "TM").then_some(vr);
        let range = if temporal.is_some() && !value.is_empty() {
            let (lower, upper) = value.split_once('-').unwrap_or((value, value));
            let lower = bound(lower, vr, false)
                .ok_or_else(|| bad("Invalid DICOM date/time matching value"))?;
            let upper = bound(upper, vr, true)
                .ok_or_else(|| bad("Invalid DICOM date/time matching value"))?;
            if !lower.is_empty() && !upper.is_empty() && lower > upper {
                return Err(bad("Reversed DICOM date/time range"));
            }
            Some((lower, upper))
        } else {
            None
        };
        let mut patterns = vec![];
        let numeric = if matches!(vr, "IS" | "US") && !value.is_empty() {
            Some(
                value
                    .split('\\')
                    .map(|v| {
                        v.trim()
                            .parse::<i64>()
                            .map_err(|_| bad("Invalid DICOM numeric matching value"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            None
        };
        let names = if vr == "PN" && !value.is_empty() {
            value
                .split('\\')
                .map(PersonPattern::new)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };
        if !value.is_empty() && range.is_none() && numeric.is_none() && names.is_empty() {
            let parts = if vr == "UI" {
                value.split(',').collect::<Vec<_>>()
            } else {
                value.split('\\').collect::<Vec<_>>()
            };
            for part in parts {
                if vr == "UI" {
                    crate::dicom::validate_uid(part)
                        .map_err(|_| bad("Invalid DICOM UID matching value"))?;
                }
                if vr == "SQ" {
                    return Err(bad("Use a sequence item attribute for DICOM matching"));
                }
                patterns.push(pattern(part, false)?);
            }
        }
        Ok(Self {
            path,
            patterns,
            range,
            temporal,
            numeric,
            names,
        })
    }
    pub(super) fn matches(&self, row: &Value) -> bool {
        self.matches_at(row, &self.path)
    }
    fn matches_at(&self, row: &Value, path: &[String]) -> bool {
        if !self.names.is_empty() {
            if let Some(values) = row[&path[0]]["Value"].as_array() {
                return values
                    .iter()
                    .any(|value| self.names.iter().any(|name| name.matches(value)));
            }
            return self.names.iter().any(|name| name.matches(&Value::Null));
        }
        if self.patterns.is_empty() && self.range.is_none() && self.numeric.is_none() {
            return true;
        }
        let values = strings(row, path);
        if values.is_empty() {
            return self.patterns.iter().any(|pattern| pattern.is_match(""));
        }
        values.iter().any(|value| {
            if let Some((lower, upper)) = &self.range {
                let vr = self.temporal.expect("temporal range");
                let Some(first) = bound(value, vr, false) else {
                    return false;
                };
                let Some(last) = bound(value, vr, true) else {
                    return false;
                };
                (lower.is_empty() || last >= *lower) && (upper.is_empty() || first <= *upper)
            } else if let Some(values) = &self.numeric {
                value
                    .trim()
                    .parse::<i64>()
                    .is_ok_and(|v| values.contains(&v))
            } else {
                self.patterns.iter().any(|p| p.is_match(value))
            }
        })
    }
}
pub(super) fn all(filters: &[Filter], row: &Value) -> bool {
    let mut sequences = std::collections::BTreeMap::<&str, Vec<&Filter>>::new();
    for filter in filters {
        if filter.patterns.is_empty()
            && filter.range.is_none()
            && filter.numeric.is_none()
            && filter.names.is_empty()
        {
            continue;
        }
        if filter.path.len() == 1 {
            if !filter.matches(row) {
                return false;
            }
        } else {
            sequences.entry(&filter.path[0]).or_default().push(filter);
        }
    }
    sequences.into_iter().all(|(tag, filters)| {
        row[tag]["Value"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                filters
                    .iter()
                    .all(|filter| filter.matches_at(item, &filter.path[1..]))
            })
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn dicom_matching_handles_precision_names_numeric_values_and_sequences() {
        let row = json!({"00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe, Jane^A"}]},"00080020":{"vr":"DA","Value":["20260228"]},"00080030":{"vr":"TM","Value":["123059.123456"]},"00200013":{"vr":"IS","Value":["001"]},"00400275":{"vr":"SQ","Value":[{"00401001":{"vr":"SH","Value":["REQ-1"]}}]}});
        for (key, value) in [
            ("PatientName", "doe, jane*"),
            ("StudyDate", "20260201-20260228"),
            ("StudyTime", "1230"),
            ("InstanceNumber", "1"),
            ("RequestAttributesSequence.RequestedProcedureID", "REQ-?"),
        ] {
            assert!(Filter::new(key, value).unwrap().matches(&row), "{key}");
        }
        assert!(!Filter::new("StudyTime", "1231-").unwrap().matches(&row));
        assert!(Filter::new("StudyDate", "20260230").is_err());
        assert!(Filter::new("StudyTime", "2500").is_err());
    }
    #[test]
    fn names_match_components_and_sequence_keys_share_one_item() {
        let row = json!({"00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe^Jane^^^","Ideographic":"山田^太郎"}]},"00400275":{"vr":"SQ","Value":[{"00400009":{"vr":"SH","Value":["A"]},"00401001":{"vr":"SH","Value":["1"]}},{"00400009":{"vr":"SH","Value":["B"]},"00401001":{"vr":"SH","Value":["2"]}}]}});
        for value in ["DOE", "^jane", "=山田*", "Doe^Jane"] {
            assert!(
                Filter::new("PatientName", value).unwrap().matches(&row),
                "{value}"
            );
        }
        let keys = [
            Filter::new("RequestAttributesSequence.ScheduledProcedureStepID", "A").unwrap(),
            Filter::new("RequestAttributesSequence.RequestedProcedureID", "2").unwrap(),
        ];
        assert!(!all(&keys, &row));
        let keys = [
            Filter::new("RequestAttributesSequence.ScheduledProcedureStepID", "B").unwrap(),
            Filter::new("RequestAttributesSequence.RequestedProcedureID", "2").unwrap(),
        ];
        assert!(all(&keys, &row));
        assert!(all(
            &[Filter::new("RequestAttributesSequence.RequestedProcedureID", "").unwrap()],
            &json!({})
        ));
    }
}
