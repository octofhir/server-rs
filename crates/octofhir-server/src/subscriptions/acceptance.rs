//! Whether the server can serve a Subscription, checked when it is created or updated: the
//! Subscriptions framework lets a server reject what it does not implement rather than accept it
//! and fail later. A Subscription the server cannot serve is refused with the reason (422).
//!
//! Checked: the topic is one this server has; the channel type is implemented (no `message`,
//! `email` only with a provider); a rest-hook endpoint is an http(s) URL the outbound policy
//! allows; the payload is JSON (this server writes no XML) and its content is `empty`, `id-only`
//! or `full-resource`; every filter names a resource type of the topic and a filter key the
//! server can evaluate.

use serde_json::{Value, json};

use super::SubscriptionState;
use super::hook::channel_problem;

const PAYLOAD_CONTENT: &str =
    "http://hl7.org/fhir/uv/subscriptions-backport/StructureDefinition/backport-payload-content";

/// Why the server cannot serve `subscription`, or `None`. Subscriptions that are not
/// `requested` or `active` (off, error, entered-in-error) are not checked: they are not served.
pub fn problem(state: &SubscriptionState, subscription: &Value) -> Option<String> {
    if !state.enabled {
        return None;
    }
    if !matches!(
        subscription["status"].as_str(),
        None | Some("requested" | "active")
    ) {
        return None;
    }
    let mut copy = subscription.clone();
    if copy.get("id").is_none() {
        copy["id"] = json!("new");
    }
    let parsed = match state.subscription_manager.parse_subscription(&copy) {
        Ok(parsed) => parsed,
        Err(error) => return Some(error.to_string()),
    };
    let Some(topic) = state.topic_registry.get_topic(&parsed.topic_url) else {
        return Some(format!(
            "Unknown SubscriptionTopic {}: no active topic has this URL",
            parsed.topic_url
        ));
    };
    if let Some(problem) = channel_problem(&parsed.channel, state.email_configured, &state.egress) {
        return Some(problem);
    }
    if let Some(problem) = payload_problem(subscription) {
        return Some(problem);
    }
    let topic_types: Vec<&str> = topic
        .resource_triggers
        .iter()
        .map(|t| t.resource_type.as_str())
        .collect();
    for filter in &parsed.filter_by {
        let types: Vec<&str> = match filter.resource_type.as_deref() {
            Some(resource_type) if !topic_types.contains(&resource_type) => {
                return Some(format!(
                    "The filter `{resource_type}?{}={}` is for {resource_type}, which the topic {} has no events for",
                    filter.filter_parameter, filter.value, topic.url
                ));
            }
            Some(resource_type) => vec![resource_type],
            None => topic_types.clone(),
        };
        for resource_type in types {
            if let Some(problem) = state.event_matcher.search().unsupported(
                Some(&topic),
                resource_type,
                &filter.filter_parameter,
                filter.modifier.as_deref(),
            ) {
                return Some(format!("Filter: {problem}"));
            }
        }
    }
    None
}

/// The payload's MIME type (R4 `channel.payload`, R5 `contentType`) must be JSON, and its
/// content (R4 `backport-payload-content`, R5 `content`) one of the three the IG defines.
fn payload_problem(subscription: &Value) -> Option<String> {
    let channel = &subscription["channel"];
    let mime = channel["payload"]
        .as_str()
        .or_else(|| subscription["contentType"].as_str());
    if let Some(mime) = mime {
        let base = mime.split(';').next().unwrap_or_default().trim();
        if !matches!(base, "application/fhir+json" | "application/json") {
            return Some(format!(
                "The payload type {mime} is not supported: this server sends application/fhir+json"
            ));
        }
    }
    let content = channel["_payload"]["extension"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["url"] == PAYLOAD_CONTENT)
        .and_then(|e| e["valueCode"].as_str())
        .or_else(|| subscription["content"].as_str());
    match content {
        None | Some("empty" | "id-only" | "full-resource") => None,
        Some(other) => Some(format!(
            "The payload content {other} is not one of empty, id-only, full-resource"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_must_be_json_with_a_known_content() {
        let subscription = |payload: &str, content: &str| {
            json!({"channel": {"payload": payload, "_payload": {"extension": [
                {"url": PAYLOAD_CONTENT, "valueCode": content}
            ]}}})
        };
        assert_eq!(
            payload_problem(&subscription("application/fhir+json", "id-only")),
            None
        );
        assert_eq!(
            payload_problem(&subscription(
                "application/fhir+json; fhirVersion=4.0",
                "empty"
            )),
            None
        );
        assert!(payload_problem(&subscription("application/fhir+xml", "id-only")).is_some());
        assert!(payload_problem(&subscription("application/fhir+json", "summary")).is_some());
    }
}
