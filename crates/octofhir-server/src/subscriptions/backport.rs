//! The R4 form of subscription status (Subscriptions Backport IG).
//!
//! R4 has no `SubscriptionStatus`: the backport carries it as a `Parameters` resource
//! (`backport-subscription-status-r4`), first in a notification `Bundle` of type `history`
//! (`backport-subscription-notification-r4`). Everything is built in the R5 shape and turned
//! into this one at the edge when the server runs R4.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value, json};

static R4: AtomicBool = AtomicBool::new(false);

/// Use the R4 form from now on.
pub fn use_r4_form(on: bool) {
    R4.store(on, Ordering::Relaxed);
}

/// A `SubscriptionStatus` in the server's form.
pub fn status(status: Value) -> Value {
    if R4.load(Ordering::Relaxed) {
        to_parameters(&status)
    } else {
        status
    }
}

/// A notification `Bundle` in the server's form.
pub fn notification(mut bundle: Value) -> Value {
    if !R4.load(Ordering::Relaxed) {
        return bundle;
    }
    if bundle["type"] == "subscription-notification" {
        bundle["type"] = json!("history");
    }
    if let Some(first) = bundle["entry"].get_mut(0)
        && first["resource"]["resourceType"] == "SubscriptionStatus"
    {
        first["resource"] = to_parameters(&first["resource"]);
    }
    bundle
}

/// An extension of another FHIR version on a Subscription, as `http://hl7.org/fhir/5.0/…`: the
/// backport describes these elements itself, and an R4 server SHOULD refuse what it does not
/// implement.
pub fn cross_version_extension(subscription: &Value) -> Option<String> {
    match subscription {
        Value::Object(object) => {
            let extensions = object.get("extension").and_then(Value::as_array);
            for extension in extensions.into_iter().flatten() {
                if let Some(url) = extension["url"].as_str()
                    && is_cross_version(url)
                {
                    return Some(url.to_owned());
                }
            }
            object.values().find_map(cross_version_extension)
        }
        Value::Array(items) => items.iter().find_map(cross_version_extension),
        _ => None,
    }
}

fn is_cross_version(url: &str) -> bool {
    url.strip_prefix("http://hl7.org/fhir/")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|version| {
            let mut parts = version.split('.');
            parts
                .next()
                .is_some_and(|major| major.parse::<u8>().is_ok())
                && parts
                    .next()
                    .is_some_and(|minor| minor.parse::<u8>().is_ok())
        })
}

/// `backport-subscription-status-r4` from an R5 `SubscriptionStatus`.
fn to_parameters(status: &Value) -> Value {
    let mut parameter = Vec::new();
    let mut add = |name: &str, kind: &str, value: &Value| {
        if !value.is_null() {
            parameter.push(json!({"name": name, kind: value}));
        }
    };
    add("subscription", "valueReference", &status["subscription"]);
    add("topic", "valueCanonical", &status["topic"]);
    add("status", "valueCode", &status["status"]);
    add("type", "valueCode", &status["type"]);
    add(
        "events-since-subscription-start",
        "valueString",
        &status["eventsSinceSubscriptionStart"],
    );
    for event in status["notificationEvent"].as_array().into_iter().flatten() {
        let mut part = Vec::new();
        let mut put = |name: &str, kind: &str, value: &Value| {
            if !value.is_null() {
                part.push(json!({"name": name, kind: value}));
            }
        };
        put("event-number", "valueString", &event["eventNumber"]);
        put("timestamp", "valueInstant", &event["timestamp"]);
        put("focus", "valueReference", &event["focus"]);
        for context in event["additionalContext"].as_array().into_iter().flatten() {
            put("additional-context", "valueReference", context);
        }
        parameter.push(json!({"name": "notification-event", "part": part}));
    }
    for error in status["error"].as_array().into_iter().flatten() {
        parameter.push(json!({"name": "error", "valueCodeableConcept": error}));
    }
    let mut parameters = Map::new();
    parameters.insert("resourceType".into(), json!("Parameters"));
    if let Some(id) = status.get("id") {
        parameters.insert("id".into(), id.clone());
    }
    parameters.insert(
        "meta".into(),
        json!({"profile": ["http://hl7.org/fhir/uv/subscriptions-backport/StructureDefinition/backport-subscription-status-r4"]}),
    );
    parameters.insert("parameter".into(), Value::Array(parameter));
    Value::Object(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_of_another_version_are_found_anywhere() {
        let subscription = json!({"resourceType": "Subscription", "_criteria": {"extension": [{
            "url": "http://hl7.org/fhir/5.0/subscriptions-backport/StructureDefinition/backport-filter-criteria",
            "valueString": "Encounter.patient=Patient/123",
        }]}});
        assert!(cross_version_extension(&subscription).is_some());
        let backport = json!({"resourceType": "Subscription", "_criteria": {"extension": [{
            "url": "http://hl7.org/fhir/uv/subscriptions-backport/StructureDefinition/backport-filter-criteria",
            "valueString": "Encounter.patient=Patient/123",
        }]}});
        assert!(cross_version_extension(&backport).is_none());
    }

    #[test]
    fn a_status_becomes_backport_parameters() {
        let status = json!({
            "resourceType": "SubscriptionStatus",
            "status": "active",
            "type": "event-notification",
            "eventsSinceSubscriptionStart": "3",
            "subscription": {"reference": "Subscription/s"},
            "topic": "http://example.org/topic",
            "notificationEvent": [{
                "eventNumber": "3",
                "timestamp": "2026-11-07T10:00:00Z",
                "focus": {"reference": "Encounter/e"},
            }],
        });
        let parameters = to_parameters(&status);
        assert_eq!(parameters["resourceType"], "Parameters");
        let names: Vec<&str> = parameters["parameter"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "subscription",
                "topic",
                "status",
                "type",
                "events-since-subscription-start",
                "notification-event"
            ]
        );
        let event = &parameters["parameter"][5]["part"];
        assert_eq!(
            event[0],
            json!({"name": "event-number", "valueString": "3"})
        );
        assert_eq!(
            event[2],
            json!({"name": "focus", "valueReference": {"reference": "Encounter/e"}})
        );
    }
}
