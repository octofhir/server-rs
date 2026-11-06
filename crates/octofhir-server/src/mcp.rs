//! Model Context Protocol endpoint: `POST /mcp`, Streamable HTTP.
//!
//! Each tool is a request to this server's own HTTP stack, carrying the caller's
//! credentials. There is no second authorization path: SMART scopes, access policies,
//! validation and audit apply to an agent exactly as to any other client, and a tool can
//! do no more than its token can over REST.
//!
//! Tools are shaped for clinical questions rather than one per REST interaction. Writes
//! are off unless `mcp.allow_writes` is set, and each one is recorded with the
//! `AIAST` label and a Provenance per the AI Transparency IG.

use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::server::AppState;

/// Newest first. The two older revisions keep the `initialize` handshake, which clients
/// still in the field depend on.
const SUPPORTED_VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const HEADER_MISMATCH: i64 = -32001;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// How long `tools/list` may be cached: the list only changes with the configuration.
const TOOLS_TTL: Duration = Duration::from_secs(300);

const AI_SECURITY_SYSTEM: &str = "http://terminology.hl7.org/CodeSystem/v3-ObservationValue";
const AI_PROVENANCE: &str =
    "http://hl7.org/fhir/uv/aitransparency/StructureDefinition/AI-Provenance";
const AI_DEVICE: &str = "http://hl7.org/fhir/uv/aitransparency/StructureDefinition/AI-Device";
const AI_DEVICE_TYPES: &str = "http://hl7.org/fhir/uv/aitransparency/CodeSystem/AIdeviceTypeCS";

const INSTRUCTIONS: &str = "FHIR server tools. Start from get_capabilities to see resource \
types and search parameters. Prefer patient_summary over many searches when the question is \
about one patient. Results are limited to what the access token may read.";

fn tools(state: &AppState) -> Vec<Value> {
    let object = |properties: Value, required: &[&str]| json!({"type": "object", "properties": properties, "required": required});
    let text = |description: &str| json!({"type": "string", "description": description});
    let read_only = json!({"readOnlyHint": true, "openWorldHint": false});
    let mut tools = vec![
        json!({"name": "evaluate_fhirpath", "title": "Evaluate FHIRPath",
            "description": "Evaluate a FHIRPath expression, against a resource when one is given.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "expression": text("FHIRPath expression"),
                "resource": {"type": "object", "description": "FHIR resource to evaluate against"},
            }), &["expression"])}),
        json!({"name": "evaluate_measure", "title": "Evaluate a measure",
            "description": "Score a Measure for a period, for one Patient, a Group, or everyone \
the token can read. Returns a MeasureReport.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "measureId": text("Measure id"),
                "periodStart": text("Start of the period, for example 2024-01-01"),
                "periodEnd": text("End of the period, for example 2024-12-31"),
                "subject": text("Patient/id or Group/id"),
            }), &["measureId", "periodStart", "periodEnd"])}),
        json!({"name": "get_capabilities", "title": "Server capabilities",
            "description": "List the resource types, search parameters and operations of this server.",
            "annotations": read_only,
            "inputSchema": object(json!({}), &[])}),
        json!({"name": "lookup_code", "title": "Look up a code",
            "description": "Get the display and properties of a code in a code system.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "system": text("Code system URL"),
                "code": text("The code"),
            }), &["system", "code"])}),
        json!({"name": "patient_summary", "title": "Patient summary",
            "description": "International Patient Summary of one patient: problems, allergies, \
medications, results, immunizations and more, as a FHIR document.",
            "annotations": read_only,
            "inputSchema": object(json!({"patientId": text("Patient id")}), &["patientId"])}),
        json!({"name": "read_resource", "title": "Read a resource",
            "description": "Read one resource by type and id.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "resourceType": text("FHIR resource type"),
                "id": text("Resource id"),
            }), &["resourceType", "id"])}),
        json!({"name": "search_resources", "title": "Search resources",
            "description": "FHIR search on one resource type. Returns a Bundle; follow its \
next link by calling this tool with the link's query.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "resourceType": text("FHIR resource type"),
                "query": text("Search query, for example patient=123&status=final&_count=20"),
            }), &["resourceType"])}),
        json!({"name": "validate_code", "title": "Check a code",
            "description": "Check whether a code is in a value set, and say which definitions decided.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "valueSet": text("ValueSet canonical URL"),
                "system": text("Code system URL"),
                "code": text("The code"),
            }), &["valueSet", "code"])}),
        json!({"name": "validate_resource", "title": "Validate a resource",
            "description": "Validate a resource against its base definition or a profile, without storing it.",
            "annotations": read_only,
            "inputSchema": object(json!({
                "resource": {"type": "object", "description": "FHIR resource"},
                "profile": text("StructureDefinition canonical URL"),
            }), &["resource"])}),
    ];
    if state.config.mcp.allow_writes {
        tools.push(json!({"name": "write_resource", "title": "Store a resource",
            "description": "Create or update a resource. It is validated, labelled as AI asserted, \
and recorded with a Provenance naming this client.",
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
            "inputSchema": object(json!({
                "resource": {"type": "object", "description": "FHIR resource; with an id it updates"},
            }), &["resource"])}));
    }
    tools
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message.into()});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn reply(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// The request's protocol version from its header and `_meta`, which must agree.
fn protocol_version<'a>(
    headers: &'a HeaderMap,
    request: &'a Value,
) -> Result<Option<&'a str>, (i64, String)> {
    let header = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok());
    let meta = request["params"]["_meta"][META_VERSION].as_str();
    if let (Some(header), Some(meta)) = (header, meta)
        && header != meta
    {
        return Err((
            HEADER_MISMATCH,
            "MCP-Protocol-Version does not match the request".into(),
        ));
    }
    let version = meta.or(header);
    if let Some(version) = version
        && !SUPPORTED_VERSIONS.contains(&version)
    {
        return Err((
            UNSUPPORTED_PROTOCOL_VERSION,
            format!("Unsupported protocol version {version}"),
        ));
    }
    Ok(version)
}

/// `Origin` guards against DNS rebinding: a browser page may only reach this endpoint
/// from the host it is served on.
fn origin_allowed(headers: &HeaderMap, base_url: &str) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(host) = origin
        .to_str()
        .ok()
        .and_then(|origin| url::Url::parse(origin).ok())
        .and_then(|url| {
            let host = url.host_str()?.to_owned();
            Some(
                url.port()
                    .map_or(host.clone(), |port| format!("{host}:{port}")),
            )
        })
    else {
        return false;
    };
    let same_as = |candidate: Option<&str>| candidate == Some(host.as_str());
    same_as(
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok()),
    ) || url::Url::parse(base_url).is_ok_and(|base| {
        let port = base
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        base.host_str()
            .is_some_and(|name| format!("{name}{port}") == host)
    })
}

fn unauthorized(state: &AppState) -> Response {
    let base = state.base_url.trim_end_matches('/');
    let challenge =
        format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\"");
    let mut response = reply(
        StatusCode::UNAUTHORIZED,
        json!({"error": "unauthorized", "error_description": "A bearer token is required"}),
    );
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

/// `GET /.well-known/oauth-protected-resource/mcp` (RFC 9728): how an MCP client finds
/// the authorization server.
pub async fn protected_resource(State(state): State<AppState>) -> Response {
    let base = state.base_url.trim_end_matches('/');
    let body = json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [state.config.auth.issuer],
        "bearer_methods_supported": ["header"],
    });
    (StatusCode::OK, Json(body)).into_response()
}

/// `POST /mcp`
pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !state.config.mcp.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !origin_allowed(&headers, &state.base_url) {
        return reply(
            StatusCode::FORBIDDEN,
            rpc_error(&Value::Null, INVALID_REQUEST, "Origin not allowed", None),
        );
    }
    if !headers.contains_key(header::AUTHORIZATION) && !state.config.auth.policy.anonymous_access {
        return unauthorized(&state);
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = request["method"].as_str() else {
        return reply(
            StatusCode::BAD_REQUEST,
            rpc_error(&id, INVALID_REQUEST, "A JSON-RPC request is required", None),
        );
    };
    if headers
        .get("mcp-method")
        .is_some_and(|value| value.to_str().ok() != Some(method))
    {
        return reply(
            StatusCode::BAD_REQUEST,
            rpc_error(
                &id,
                HEADER_MISMATCH,
                "Mcp-Method does not match the request",
                None,
            ),
        );
    }
    let version = match protocol_version(&headers, &request) {
        Ok(version) => version,
        Err((code, message)) => {
            let data = (code == UNSUPPORTED_PROTOCOL_VERSION)
                .then(|| json!({"supported": SUPPORTED_VERSIONS}));
            return reply(StatusCode::BAD_REQUEST, rpc_error(&id, code, message, data));
        }
    };
    // A notification asks for nothing back.
    if request.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let server_info = json!({"name": "OctoFHIR", "version": env!("CARGO_PKG_VERSION")});
    let capabilities = json!({"tools": {}});
    let cache = json!({"ttlMs": TOOLS_TTL.as_millis() as u64, "cacheScope": "public"});
    let result = match method {
        "server/discover" => json!({
            "resultType": "complete",
            "supportedVersions": SUPPORTED_VERSIONS,
            "capabilities": capabilities,
            "serverInfo": server_info,
            "instructions": INSTRUCTIONS,
            "ttlMs": cache["ttlMs"],
            "cacheScope": cache["cacheScope"],
        }),
        "initialize" => {
            let requested = request["params"]["protocolVersion"].as_str();
            let agreed = requested
                .filter(|version| SUPPORTED_VERSIONS.contains(version))
                .unwrap_or(SUPPORTED_VERSIONS[1]);
            json!({
                "protocolVersion": agreed,
                "capabilities": capabilities,
                "serverInfo": server_info,
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "resultType": "complete",
            "tools": tools(&state),
            "ttlMs": cache["ttlMs"],
            "cacheScope": cache["cacheScope"],
        }),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or_default();
            if headers
                .get("mcp-name")
                .is_some_and(|value| value.to_str().ok() != Some(name))
                && version == Some(SUPPORTED_VERSIONS[0])
            {
                return reply(
                    StatusCode::BAD_REQUEST,
                    rpc_error(
                        &id,
                        HEADER_MISMATCH,
                        "Mcp-Name does not match the request",
                        None,
                    ),
                );
            }
            let arguments = &request["params"]["arguments"];
            match call_tool(&state, &headers, name, arguments).await {
                Ok(ToolResult::Done(status, body)) => tool_result(status, body),
                // Credentials are for the transport to answer, so the client can renew them.
                Ok(ToolResult::Unauthorized) => return unauthorized(&state),
                Err(message) => {
                    return reply(
                        StatusCode::OK,
                        rpc_error(&id, INVALID_PARAMS, message, None),
                    );
                }
            }
        }
        _ => {
            return reply(
                StatusCode::NOT_FOUND,
                rpc_error(
                    &id,
                    METHOD_NOT_FOUND,
                    format!("Unknown method {method}"),
                    None,
                ),
            );
        }
    };
    reply(StatusCode::OK, rpc_result(&id, result))
}

/// A tool's response in MCP form. A FHIR error is a tool error the agent can read.
fn tool_result(status: StatusCode, body: Value) -> Value {
    let text = serde_json::to_string(&body).unwrap_or_default();
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "structuredContent": body,
        "isError": !status.is_success(),
    })
}

enum ToolResult {
    Done(StatusCode, Value),
    Unauthorized,
}

fn required<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments[name]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

/// A resource type from a tool argument.
fn type_segment<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    let value = required(arguments, name)?;
    if value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(value)
    } else {
        Err(format!("{name} is not a resource type"))
    }
}

/// A resource id from a tool argument. It becomes a path segment, so it may not be one
/// that reaches another endpoint.
fn id_segment<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    let value = required(arguments, name)?;
    if octofhir_core::validate_id(value).is_ok() && !value.bytes().all(|byte| byte == b'.') {
        Ok(value)
    } else {
        Err(format!("{name} is not a resource id"))
    }
}

fn parameters(parameter: Value) -> Value {
    json!({"resourceType": "Parameters", "parameter": parameter})
}

async fn call_tool(
    state: &AppState,
    headers: &HeaderMap,
    name: &str,
    arguments: &Value,
) -> Result<ToolResult, String> {
    let (method, path, body) = match name {
        "get_capabilities" => (
            Method::GET,
            "/fhir/metadata?_summary=false".to_owned(),
            None,
        ),
        "read_resource" => (
            Method::GET,
            format!(
                "/fhir/{}/{}",
                type_segment(arguments, "resourceType")?,
                id_segment(arguments, "id")?
            ),
            None,
        ),
        "search_resources" => {
            let query = arguments["query"].as_str().unwrap_or_default();
            let query = query.trim_start_matches('?');
            let separator = if query.is_empty() { "" } else { "?" };
            (
                Method::GET,
                format!(
                    "/fhir/{}{separator}{query}",
                    type_segment(arguments, "resourceType")?
                ),
                None,
            )
        }
        "patient_summary" => (
            Method::GET,
            format!(
                "/fhir/Patient/{}/$summary",
                id_segment(arguments, "patientId")?
            ),
            None,
        ),
        "lookup_code" => (
            Method::POST,
            "/fhir/CodeSystem/$lookup".to_owned(),
            Some(parameters(json!([
                {"name": "system", "valueUri": required(arguments, "system")?},
                {"name": "code", "valueCode": required(arguments, "code")?},
            ]))),
        ),
        "validate_code" => {
            let mut list = vec![
                json!({"name": "url", "valueUri": required(arguments, "valueSet")?}),
                json!({"name": "code", "valueCode": required(arguments, "code")?}),
                json!({"name": "_explain", "valueBoolean": true}),
            ];
            if let Some(system) = arguments["system"].as_str() {
                list.push(json!({"name": "system", "valueUri": system}));
            } else {
                list.push(json!({"name": "inferSystem", "valueBoolean": true}));
            }
            (
                Method::POST,
                "/fhir/ValueSet/$validate-code".to_owned(),
                Some(parameters(json!(list))),
            )
        }
        "validate_resource" => {
            let resource = arguments
                .get("resource")
                .filter(|resource| resource.is_object())
                .ok_or("resource is required")?;
            let resource_type = resource["resourceType"]
                .as_str()
                .filter(|name| name.bytes().all(|byte| byte.is_ascii_alphanumeric()))
                .ok_or("resource has no resourceType")?;
            let mut list = vec![json!({"name": "resource", "resource": resource})];
            if let Some(profile) = arguments["profile"].as_str() {
                list.push(json!({"name": "profile", "valueCanonical": profile}));
            }
            (
                Method::POST,
                format!("/fhir/{resource_type}/$validate"),
                Some(parameters(json!(list))),
            )
        }
        "evaluate_measure" => {
            let mut list = vec![
                json!({"name": "periodStart", "valueDate": required(arguments, "periodStart")?}),
                json!({"name": "periodEnd", "valueDate": required(arguments, "periodEnd")?}),
            ];
            if let Some(subject) = arguments["subject"].as_str() {
                list.push(json!({"name": "subject", "valueString": subject}));
            }
            (
                Method::POST,
                format!(
                    "/fhir/Measure/{}/$evaluate-measure",
                    id_segment(arguments, "measureId")?
                ),
                Some(parameters(json!(list))),
            )
        }
        "evaluate_fhirpath" => {
            let mut list = vec![json!({
                "name": "expression", "valueString": required(arguments, "expression")?,
            })];
            if let Some(resource) = arguments.get("resource").filter(|r| r.is_object()) {
                list.push(json!({"name": "resource", "resource": resource}));
            }
            (
                Method::POST,
                "/fhir/$fhirpath".to_owned(),
                Some(parameters(json!(list))),
            )
        }
        "write_resource" if state.config.mcp.allow_writes => (
            Method::POST,
            "/fhir".to_owned(),
            Some(attributed_write(arguments, headers)?),
        ),
        _ => return Err(format!("Unknown tool {name}")),
    };
    dispatch(state, headers, method, &path, body).await
}

/// `Sub` of the bearer token, as far as it can be read: only a label on the record, the
/// real check of the token happens when the transaction is executed.
fn token_client(headers: &HeaderMap) -> String {
    use base64::Engine as _;
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|token| token.split('.').nth(1))
        .and_then(|claims| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(claims)
                .ok()
        })
        .and_then(|claims| serde_json::from_slice::<Value>(&claims).ok())
        .and_then(|claims| {
            claims["client_id"]
                .as_str()
                .or_else(|| claims["sub"].as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// A transaction storing the resource under the AI label with its Provenance and the
/// Device standing for the agent (AI Transparency IG).
fn attributed_write(arguments: &Value, headers: &HeaderMap) -> Result<Value, String> {
    let mut resource = arguments
        .get("resource")
        .filter(|resource| resource.is_object())
        .ok_or("resource is required")?
        .clone();
    let resource_type = resource["resourceType"]
        .as_str()
        .filter(|name| name.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .ok_or("resource has no resourceType")?
        .to_owned();
    let label = json!({"system": AI_SECURITY_SYSTEM, "code": "AIAST",
        "display": "Artificial Intelligence asserted"});
    match resource["meta"]["security"].as_array_mut() {
        Some(labels) => labels.push(label),
        None => resource["meta"]["security"] = json!([label]),
    }
    let request = match resource["id"].as_str() {
        Some(id) => json!({"method": "PUT", "url": format!("{resource_type}/{id}")}),
        None => json!({"method": "POST", "url": resource_type}),
    };
    let client = token_client(headers);
    let target = format!("urn:uuid:{}", Uuid::new_v4());
    let device = format!("urn:uuid:{}", Uuid::new_v4());
    let identifier = "urn:octofhir:mcp-client";
    Ok(
        json!({"resourceType": "Bundle", "type": "transaction", "entry": [
            {"fullUrl": target, "resource": resource, "request": request},
            {"fullUrl": device,
             "resource": {"resourceType": "Device",
                "meta": {"profile": [AI_DEVICE]},
                "identifier": [{"system": identifier, "value": client}],
                "type": {"coding": [{"system": AI_DEVICE_TYPES, "code": "Artificial-Intelligence",
                    "display": "All kinds of Artificial Intelligence"}]},
                "deviceName": [{"name": client, "type": "user-friendly-name"}]},
             "request": {"method": "POST", "url": "Device",
                "ifNoneExist": format!("identifier={identifier}|{client}")}},
            {"resource": {"resourceType": "Provenance",
                "meta": {"profile": [AI_PROVENANCE]},
                "target": [{"reference": target}],
                "recorded": chrono::Utc::now().to_rfc3339(),
                "reason": [{"coding": [{"system": AI_SECURITY_SYSTEM, "code": "AIAST"}]}],
                "agent": [{"type": {"coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/provenance-participant-type",
                        "code": "author", "display": "Author"}]},
                    "who": {"reference": device}}]},
             "request": {"method": "POST", "url": "Provenance"}},
        ]}),
    )
}

/// Sends the request through the server's full stack as the caller.
async fn dispatch(
    state: &AppState,
    headers: &HeaderMap,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<ToolResult, String> {
    let router = state
        .internal_router
        .load_full()
        .ok_or("The server is not running")?;
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ACCEPT, "application/fhir+json");
    if let Some(authorization) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/fhir+json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .map_err(|_| "This tool call cannot be expressed as a request")?;
    let response = Router::clone(&router)
        .oneshot(request)
        .await
        .map_err(|_| "The server did not answer")?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return Ok(ToolResult::Unauthorized);
    }
    let bytes = to_bytes(
        response.into_body(),
        state.config.search.include_limits.max_bytes,
    )
    .await
    .map_err(|_| "The result is too large; narrow the request")?;
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        json!({"resourceType": "OperationOutcome", "issue": [{
            "severity": "error", "code": "exception",
            "diagnostics": String::from_utf8_lossy(&bytes),
        }]})
    });
    Ok(ToolResult::Done(status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn versions_must_agree_and_be_supported() {
        let body = json!({"params": {"_meta": {META_VERSION: "2026-07-28"}}});
        assert_eq!(
            protocol_version(&headers(&[("mcp-protocol-version", "2026-07-28")]), &body),
            Ok(Some("2026-07-28"))
        );
        assert_eq!(
            protocol_version(&headers(&[("mcp-protocol-version", "2025-11-25")]), &body)
                .unwrap_err()
                .0,
            HEADER_MISMATCH
        );
        assert_eq!(
            protocol_version(
                &headers(&[("mcp-protocol-version", "1999-01-01")]),
                &json!({})
            )
            .unwrap_err()
            .0,
            UNSUPPORTED_PROTOCOL_VERSION
        );
        assert_eq!(protocol_version(&HeaderMap::new(), &json!({})), Ok(None));
    }

    #[test]
    fn an_origin_must_be_this_server() {
        let base = "https://fhir.example.org";
        assert!(origin_allowed(&HeaderMap::new(), base));
        assert!(origin_allowed(&headers(&[("origin", base)]), base));
        assert!(origin_allowed(
            &headers(&[
                ("origin", "http://localhost:8888"),
                ("host", "localhost:8888")
            ]),
            base
        ));
        assert!(!origin_allowed(
            &headers(&[
                ("origin", "https://evil.example"),
                ("host", "fhir.example.org")
            ]),
            base
        ));
        assert!(!origin_allowed(&headers(&[("origin", "null")]), base));
    }

    #[test]
    fn arguments_cannot_reach_other_paths() {
        assert!(id_segment(&json!({"id":"abc-1.2"}), "id").is_ok());
        for id in ["..", ".", "../admin", "a/b", "a?b", "a#b", ""] {
            assert!(id_segment(&json!({ "id": id }), "id").is_err(), "{id}");
        }
        assert!(type_segment(&json!({"t":"Patient"}), "t").is_ok());
        for name in ["..", "a/b", "Patient/1"] {
            assert!(type_segment(&json!({ "t": name }), "t").is_err(), "{name}");
        }
    }

    #[test]
    fn writes_carry_the_label_a_provenance_and_the_agent() {
        let bundle = attributed_write(
            &json!({"resource":{"resourceType":"Observation","id":"o1","status":"final"}}),
            &HeaderMap::new(),
        )
        .unwrap();
        let entries = bundle["entry"].as_array().unwrap();
        assert_eq!(
            entries[0]["resource"]["meta"]["security"][0]["code"],
            "AIAST"
        );
        assert_eq!(entries[0]["request"]["method"], "PUT");
        assert_eq!(entries[1]["request"]["method"], "POST");
        assert!(entries[1]["request"]["ifNoneExist"].is_string());
        assert_eq!(
            entries[2]["resource"]["target"][0]["reference"],
            entries[0]["fullUrl"]
        );
        assert_eq!(
            entries[2]["resource"]["agent"][0]["who"]["reference"],
            entries[1]["fullUrl"]
        );
    }

    #[test]
    fn a_fhir_error_is_a_tool_error() {
        let result = tool_result(
            StatusCode::NOT_FOUND,
            json!({"resourceType":"OperationOutcome"}),
        );
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["type"], "text");
    }
}
