//! Batch writes use primary handlers, including their atomic authorization,
//! validation, reference locks, conditional matching and post-commit effects.

use axum::{
    Extension, Json,
    body::{Bytes, to_bytes},
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use octofhir_api::ApiError;
use serde_json::{Value, json};

use crate::{handlers, server::AppState, write::AuthorizedWrite};

pub(crate) async fn execute(
    state: &AppState,
    entry: &Value,
    authorization: Option<AuthorizedWrite>,
    outer_headers: &HeaderMap,
    include_resource: bool,
) -> Result<Value, ApiError> {
    let request = &entry["request"];
    let method = request["method"]
        .as_str()
        .unwrap_or("")
        .to_ascii_uppercase();
    let url = request["url"].as_str().unwrap_or("");
    let (path, query) = url
        .split_once('?')
        .map_or((url, None), |(path, query)| (path, Some(query.to_owned())));
    let mut segments = path.split('/');
    let resource_type = segments.next().unwrap_or("").to_owned();
    let id = segments.next().map(str::to_owned);
    if resource_type.is_empty() || segments.next().is_some() || id.as_deref() == Some("") {
        return Err(ApiError::bad_request(
            "Batch write URL requires Type or Type/id",
        ));
    }
    let mut headers = HeaderMap::new();
    // The Bundle controls returned content. Render the internal response once
    // and project it into an entry without performing another resource read.
    headers.insert("Prefer", HeaderValue::from_static("return=representation"));
    if let Some(value) = outer_headers.get("X-Skip-Validation") {
        headers.insert("X-Skip-Validation", value.clone());
    }
    if let Some(value) = request.get("ifMatch") {
        insert_request_header(&mut headers, header::IF_MATCH, "ifMatch", value)?;
    }
    let authorization = authorization.map(Extension);
    let response = match method.as_str() {
        "POST" if id.is_none() => {
            if let Some(value) = request.get("ifNoneExist") {
                insert_request_header(
                    &mut headers,
                    header::HeaderName::from_static("if-none-exist"),
                    "ifNoneExist",
                    value,
                )?;
            } else if let Some(query) = query {
                headers.insert(
                    "If-None-Exist",
                    HeaderValue::from_str(&query).map_err(|_| {
                        ApiError::bad_request("Invalid conditional create criterion")
                    })?,
                );
            }
            Box::pin(handlers::create_resource(
                State(state.clone()),
                Path(resource_type),
                authorization,
                headers,
                Json(resource(entry)?),
            ))
            .await?
        }
        "PUT" => {
            let payload = Json(resource(entry)?);
            match id {
                Some(id) => {
                    Box::pin(handlers::update_resource(
                        State(state.clone()),
                        Path((resource_type, id)),
                        authorization,
                        headers,
                        payload,
                    ))
                    .await?
                }
                None => {
                    Box::pin(handlers::conditional_update_resource(
                        State(state.clone()),
                        Path(resource_type),
                        authorization,
                        RawQuery(query),
                        headers,
                        payload,
                    ))
                    .await?
                }
            }
        }
        "PATCH" => {
            let (media_type, body) = crate::patch::decode_bundle_patch(&resource(entry)?)?;
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
            match id {
                Some(id) => {
                    Box::pin(handlers::patch_resource(
                        State(state.clone()),
                        Path((resource_type, id)),
                        authorization,
                        headers,
                        Bytes::from(body),
                    ))
                    .await?
                }
                None => {
                    Box::pin(handlers::conditional_patch_resource(
                        State(state.clone()),
                        Path(resource_type),
                        authorization,
                        RawQuery(query),
                        headers,
                        Bytes::from(body),
                    ))
                    .await?
                }
            }
        }
        "DELETE" => {
            if entry.get("resource").is_some() {
                return Err(ApiError::bad_request("DELETE requires an empty body"));
            }
            match id {
                Some(id) => Box::pin(handlers::delete_resource(
                    State(state.clone()),
                    Path((resource_type, id)),
                    authorization,
                    headers,
                ))
                .await?
                .into_response(),
                None => Box::pin(handlers::conditional_delete_resource(
                    State(state.clone()),
                    Path(resource_type),
                    authorization,
                    RawQuery(query),
                    headers,
                ))
                .await?
                .into_response(),
            }
        }
        _ => return Err(ApiError::bad_request("Unsupported batch write interaction")),
    };
    response_entry(state, response, include_resource).await
}

fn resource(entry: &Value) -> Result<Value, ApiError> {
    entry
        .get("resource")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("Batch write requires a resource"))
}

fn insert_request_header(
    headers: &mut HeaderMap,
    name: header::HeaderName,
    field: &str,
    value: &Value,
) -> Result<(), ApiError> {
    let value = value
        .as_str()
        .ok_or_else(|| ApiError::bad_request(format!("request.{field} must be a string")))?;
    let value = HeaderValue::from_str(value)
        .map_err(|_| ApiError::bad_request(format!("Invalid request.{field}")))?;
    headers.insert(name, value);
    Ok(())
}

async fn response_entry(
    state: &AppState,
    response: Response,
    include_resource: bool,
) -> Result<Value, ApiError> {
    let (parts, body) = response.into_parts();
    let mut entry = json!({"response":{"status":format!("{} {}", parts.status.as_u16(), parts.status.canonical_reason().unwrap_or("Success"))}});
    for (name, field) in [(header::LOCATION, "location"), (header::ETAG, "etag")] {
        if let Some(value) = parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
        {
            entry["response"][field] = json!(value);
        }
    }
    // An internal handler has already materialized the body. Avoid imposing the
    // incoming request limit after a successfully committed mutation.
    let bytes = to_bytes(body, usize::MAX)
        .await
        .map_err(|_| ApiError::internal("Cannot render batch write response"))?;
    if !bytes.is_empty() {
        let resource: Value = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::internal("Invalid batch write response"))?;
        if let Some(updated) = resource["meta"]["lastUpdated"].as_str() {
            entry["response"]["lastModified"] = json!(updated);
        }
        if let (Some(resource_type), Some(id)) =
            (resource["resourceType"].as_str(), resource["id"].as_str())
        {
            entry["fullUrl"] = json!(format!("{}/{resource_type}/{id}", state.fhir_base_url()));
        }
        if include_resource {
            entry["resource"] = resource;
        }
    }
    Ok(entry)
}
