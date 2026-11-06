//! A CQL debug trace as output parameters, in the format of the FHIR Mapping Language IG.

use octofhir_cql_eval::{TraceEvent, TraceRecorder, TraceVariable};
use serde_json::{Value, json};

use crate::data_provider::cql_value_to_json;

/// Events a `debug` evaluation keeps.
pub(crate) const MAX_TRACE_EVENTS: usize = 10_000;

/// `locator` as the 0-based rows and columns of the format: `line:column-line:column`.
fn range(locator: &str) -> Option<[usize; 4]> {
    let (start, end) = locator.split_once('-')?;
    let (start_row, start_column) = start.split_once(':')?;
    let (end_row, end_column) = end.split_once(':')?;
    Some([
        start_row.parse::<usize>().ok()?.checked_sub(1)?,
        start_column.parse::<usize>().ok()?.checked_sub(1)?,
        end_row.parse::<usize>().ok()?.checked_sub(1)?,
        end_column.parse::<usize>().ok()?.checked_sub(1)?,
    ])
}

fn value_part(value: &Value) -> Option<Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(_) => json!({"name": "value", "valueBoolean": value}),
        Value::Number(number) if number.is_f64() => json!({"name": "value", "valueDecimal": value}),
        Value::Number(_) => json!({"name": "value", "valueInteger": value}),
        Value::String(_) => json!({"name": "value", "valueString": value}),
        Value::Object(fields) if fields.contains_key("resourceType") => {
            json!({"name": "resource", "resource": value})
        }
        other => json!({"name": "jsonValue", "valueString": other.to_string()}),
    })
}

fn variable(variable: &TraceVariable) -> Value {
    let mut parts = vec![
        json!({"name": "name", "valueString": variable.name}),
        json!({"name": "contextType", "valueString": variable.context_type}),
    ];
    parts.extend(
        cql_value_to_json(&variable.value)
            .as_ref()
            .and_then(value_part),
    );
    json!({"name": "variable", "part": parts})
}

fn event(event: &TraceEvent) -> Value {
    let mut parts = vec![json!({"name": "source", "valueString": event.library})];
    if let Some([start_row, start_column, end_row, end_column]) =
        event.locator.as_deref().and_then(range)
    {
        parts.push(json!({"name": "startRow", "valueInteger": start_row}));
        parts.push(json!({"name": "startColumn", "valueInteger": start_column}));
        parts.push(json!({"name": "endRow", "valueInteger": end_row}));
        parts.push(json!({"name": "endColumn", "valueInteger": end_column}));
    }
    if !event.name.is_empty() {
        parts.push(json!({"name": "expressionPosition", "valueString": event.name}));
    }
    parts.push(json!({"name": "phase", "valueCode": event.phase.code()}));
    parts.push(json!({"name": "depth", "valueInteger": event.depth}));
    parts.extend(event.variables.iter().map(variable));
    if let Some(message) = &event.message {
        parts.push(json!({"name": "message", "part": [
            {"name": "text", "valueString": message}
        ]}));
    }
    json!({"name": "trace", "part": parts})
}

/// `traceStatus` and a `trace` per event.
pub(crate) fn parameters(recorder: &TraceRecorder, failed: bool) -> Vec<Value> {
    let status = match (failed, recorder.truncated()) {
        (true, _) => "stopped",
        (false, true) => "truncated",
        (false, false) => "complete",
    };
    let events = recorder.events();
    let mut parameters = Vec::with_capacity(events.len() + 1);
    parameters.push(json!({"name": "traceStatus", "valueCode": status}));
    parameters.extend(events.iter().map(event));
    parameters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locator_becomes_rows_and_columns_from_zero() {
        assert_eq!(range("3:1-4:9"), Some([2, 0, 3, 8]));
        assert_eq!(range("nonsense"), None);
    }

    #[test]
    fn values_take_the_part_of_their_kind() {
        assert_eq!(value_part(&Value::Null), None);
        assert_eq!(value_part(&json!(1)).unwrap()["valueInteger"], 1);
        assert_eq!(value_part(&json!(1.5)).unwrap()["valueDecimal"], 1.5);
        let patient = json!({"resourceType": "Patient"});
        assert_eq!(value_part(&patient).unwrap()["name"], "resource");
        assert_eq!(
            value_part(&json!([1, 2])).unwrap(),
            json!({"name": "jsonValue", "valueString": "[1,2]"})
        );
    }
}
