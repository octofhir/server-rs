//! Credential hashes stored inside resources.
//!
//! `User.passwordHash`, `Client.clientSecret` and `App.secret` live in the resource JSON
//! because the auth layer verifies credentials against them, and
//! `IdentityProvider.clientSecret` because the server presents it. They must never leave the
//! server, so every HTTP response is scrubbed on the way out ([`scrub_credentials_middleware`])
//! and bulk export scrubs each line it writes. An update that sends a resource back without
//! the hash keeps the stored one ([`carry_over_secrets`]); otherwise reading a user and
//! saving it again would wipe the password.

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{StatusCode, header::CONTENT_TYPE},
    middleware::Next,
    response::{IntoResponse, Response},
};
use memchr::memmem;
use serde_json::Value;
use std::sync::LazyLock;

/// Keys a password hash has been stored under (the auth model accepts both spellings).
const PASSWORD_HASH_KEYS: [&str; 2] = ["passwordHash", "password_hash"];

/// Client and App secret keys; stored values are argon2 hashes.
const SECRET_KEYS: [&str; 2] = ["clientSecret", "secret"];

/// Prefilters for the credential keys: every password key starts with `"password` and every
/// secret key ends with `ecret"`, so two substring searches cover all four. A hit only means
/// the body is parsed; [`scrub`] decides what is removed.
static PASSWORD_KEY: LazyLock<memmem::Finder<'static>> =
    LazyLock::new(|| memmem::Finder::new(b"\"password"));
static SECRET_KEY: LazyLock<memmem::Finder<'static>> =
    LazyLock::new(|| memmem::Finder::new(b"ecret\""));

/// Credentials of a `NotificationProvider`, inside its `config`.
const PROVIDER_SECRETS: [&str; 4] = ["apiKey", "smtpPassword", "botToken", "webhookSecret"];

/// Hashed credential fields per resource type.
fn secret_fields(resource_type: &str) -> &'static [&'static str] {
    match resource_type {
        "User" => &PASSWORD_HASH_KEYS,
        "Client" | "IdentityProvider" => &["clientSecret"],
        "App" => &["secret"],
        _ => &[],
    }
}

fn is_hash(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.starts_with("$argon2"))
}

/// Removes credential hashes anywhere in `value` (a resource, a Bundle, a GraphQL result…).
///
/// Password hashes are dropped under any object. `clientSecret` and `secret` are dropped only
/// when they hold an argon2 hash, so a freshly generated plain secret that is shown once stays.
/// Returns whether anything was removed.
pub fn scrub(value: &mut Value) -> bool {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            for key in PASSWORD_HASH_KEYS {
                changed |= object.remove(key).is_some();
            }
            // What the server presents to a provider cannot be hashed, so it has no mark of its
            // own; its resources do carry `resourceType`.
            match object.get("resourceType").and_then(Value::as_str) {
                Some("IdentityProvider") => changed |= object.remove("clientSecret").is_some(),
                Some("NotificationProvider") => {
                    if let Some(Value::Object(config)) = object.get_mut("config") {
                        for key in PROVIDER_SECRETS {
                            changed |= config.remove(key).is_some();
                        }
                    }
                }
                _ => {}
            }
            // Auth entities are stored without `resourceType`, so match on the hashed value.
            for key in SECRET_KEYS {
                if object.get(key).is_some_and(is_hash) {
                    object.remove(key);
                    changed = true;
                }
            }
            for nested in object.values_mut() {
                changed |= scrub(nested);
            }
            changed
        }
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |changed, item| scrub(item) | changed),
        _ => false,
    }
}

/// Fills in credential hashes an update omitted from the stored version of the resource.
///
/// A new plain `password` on a User is hashed later by the write path, so it wins.
pub fn carry_over_secrets(resource_type: &str, payload: &mut Value, stored: &Value) {
    if resource_type == "NotificationProvider" {
        if let (Some(Value::Object(incoming)), Some(Value::Object(kept))) =
            (payload.get_mut("config"), stored.get("config"))
        {
            for key in PROVIDER_SECRETS {
                if !incoming.contains_key(key)
                    && let Some(value) = kept.get(key)
                {
                    incoming.insert(key.to_owned(), value.clone());
                }
            }
        }
        return;
    }
    let fields = secret_fields(resource_type);
    let (Some(incoming), Some(stored)) = (payload.as_object_mut(), stored.as_object()) else {
        return;
    };
    if fields.iter().any(|key| incoming.contains_key(*key)) || incoming.contains_key("password") {
        return;
    }
    for key in fields {
        if let Some(value) = stored.get(*key) {
            incoming.insert((*key).to_string(), value.clone());
        }
    }
}

/// Whether updates of this type need the stored version for [`carry_over_secrets`].
pub fn has_secrets(resource_type: &str) -> bool {
    resource_type == "NotificationProvider" || !secret_fields(resource_type).is_empty()
}

/// Re-serialized `json` without credential hashes, or `None` when it had none (or isn't JSON).
fn scrub_document(json: &[u8]) -> Option<Vec<u8>> {
    let mut value = serde_json::from_slice::<Value>(json).ok()?;
    if !scrub(&mut value) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

fn mentions_credentials(body: &[u8]) -> bool {
    PASSWORD_KEY.find(body).is_some() || SECRET_KEY.find(body).is_some()
}

/// Scrubs credential hashes from JSON and NDJSON responses.
///
/// Bodies are only parsed when they contain one of the credential keys, so ordinary
/// responses cost a buffered copy and a SIMD substring search.
pub async fn scrub_credentials_middleware(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let ndjson = content_type.contains("ndjson");
    if !content_type.contains("json") {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        // Never hand out a truncated body that may have skipped the scrub.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if !mentions_credentials(&bytes) {
        return Response::from_parts(parts, Body::from(bytes));
    }

    let scrubbed = if ndjson {
        let mut out = Vec::with_capacity(bytes.len());
        for line in bytes.split(|byte| *byte == b'\n') {
            match scrub_document(line) {
                Some(scrubbed) => out.extend(scrubbed),
                None => out.extend_from_slice(line),
            }
            out.push(b'\n');
        }
        out.pop();
        Some(out)
    } else {
        scrub_document(&bytes)
    };

    match scrubbed {
        Some(body) => {
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            Response::from_parts(parts, Body::from(body))
        }
        None => Response::from_parts(parts, Body::from(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn credentials_of_a_notification_provider_stay_in() {
        let mut provider = json!({
            "resourceType": "NotificationProvider",
            "config": {"webhookUrl": "https://example.org/hook", "webhookSecret": "s", "apiKey": "k"},
        });
        assert!(scrub(&mut provider));
        assert_eq!(
            provider["config"],
            json!({"webhookUrl": "https://example.org/hook"})
        );

        let stored = json!({"config": {"webhookUrl": "https://a", "webhookSecret": "s"}});
        let mut update = json!({"config": {"webhookUrl": "https://b"}});
        carry_over_secrets("NotificationProvider", &mut update, &stored);
        assert_eq!(
            update["config"],
            json!({"webhookUrl": "https://b", "webhookSecret": "s"})
        );
    }

    #[test]
    fn the_secret_of_an_identity_provider_stays_in() {
        let mut bundle = json!({"entry": [{"resource": {
            "resourceType": "IdentityProvider",
            "clientSecret": "plain-and-needed-by-the-server",
        }}]});
        assert!(scrub(&mut bundle));
        assert!(bundle["entry"][0]["resource"].get("clientSecret").is_none());
    }

    #[test]
    fn scrubs_password_hashes_inside_bundles() {
        let mut bundle = json!({
            "resourceType": "Bundle",
            "entry": [
                {"resource": {"resourceType": "User", "id": "a", "username": "admin", "password_hash": "$argon2id$x"}},
                {"resource": {"resourceType": "User", "id": "b", "passwordHash": "$argon2id$y"}},
                {"resource": {"resourceType": "Patient", "id": "p"}}
            ]
        });
        assert!(scrub(&mut bundle));
        let text = bundle.to_string();
        assert!(!text.contains("argon2"));
        assert!(text.contains("\"username\":\"admin\""));
    }

    #[test]
    fn keeps_plain_client_secrets_but_drops_hashes() {
        let mut created = json!({"resourceType": "Client", "clientSecret": "plain-shown-once"});
        assert!(!scrub(&mut created));
        assert_eq!(created["clientSecret"], "plain-shown-once");

        let mut stored = json!({"resourceType": "App", "secret": "$argon2id$z", "name": "x"});
        assert!(scrub(&mut stored));
        assert!(stored.get("secret").is_none());
    }

    #[test]
    fn drops_hashed_secrets_even_without_resource_type() {
        // Auth entities are stored without `resourceType`.
        let mut client = json!({"clientId": "backend", "clientSecret": "$argon2id$h"});
        assert!(scrub(&mut client));
        assert!(client.get("clientSecret").is_none());

        let mut plain = json!({"resourceType": "Basic", "secret": "not-a-hash"});
        assert!(!scrub(&mut plain));
    }

    #[test]
    fn carries_stored_hash_into_updates_that_omit_it() {
        let stored =
            json!({"resourceType": "User", "username": "a", "password_hash": "$argon2id$x"});

        let mut edit = json!({"resourceType": "User", "username": "renamed"});
        carry_over_secrets("User", &mut edit, &stored);
        assert_eq!(edit["password_hash"], "$argon2id$x");

        let mut new_password = json!({"resourceType": "User", "username": "a", "password": "n3w"});
        carry_over_secrets("User", &mut new_password, &stored);
        assert!(new_password.get("password_hash").is_none());
    }
}
