//! Gateway resource types (App and CustomOperation).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::app_platform::{
    AppEndpoint, AppStatus, HttpMethod, NotificationDef, OperationPolicy, SubscriptionChannel,
    SubscriptionTrigger,
};
use octofhir_auth::middleware::AuthContext;

/// App resource - matches App Manifest format from IG.
///
/// Apps can define inline operations that are auto-reconciled to CustomOperations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "resourceType")]
    pub resource_type: String,

    /// Human-readable name of the app.
    pub name: String,

    /// Optional description of the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// App manifest API version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<u32>,

    /// App status: active, inactive, suspended.
    #[serde(default)]
    pub status: AppStatus,

    /// App secret for authentication (required, stored as hash).
    pub secret: String,

    /// Backend endpoint configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<AppEndpoint>,

    /// Inline operation definitions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<InlineOperation>,

    /// Event subscriptions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<InlineSubscription>,

    /// Resources to provision (JSON-encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<String>,

    // --- Deprecated fields for backward compatibility ---
    /// Deprecated: Use endpoint.url instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,

    /// Deprecated: Use status instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

impl App {
    /// Returns whether the app is active.
    pub fn is_active(&self) -> bool {
        // Check new status field first
        if self.status == AppStatus::Active {
            return true;
        }
        // Fall back to deprecated `active` field
        self.active.unwrap_or(false)
    }
}

/// Inline operation definition (from App.operations[]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineOperation {
    /// Unique identifier for this operation within the app.
    pub id: String,

    /// HTTP method.
    pub method: HttpMethod,

    /// URL path segments.
    pub path: Vec<String>,

    /// Operation type: "app" (default), "websocket", "proxy".
    /// - "app": Forwards to App backend endpoint
    /// - "websocket": WebSocket proxy to backend
    /// - "proxy": HTTP proxy to specified URL
    #[serde(rename = "type", default = "default_operation_type")]
    pub operation_type: String,

    /// Whether this operation is public (no authentication required).
    #[serde(default)]
    pub public: bool,

    /// Access policy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<OperationPolicy>,

    /// Whether to include raw request body bytes (base64-encoded) in AppOperationRequest.
    /// Useful for binary data, file uploads, or when the app needs the original bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_raw_body: Option<bool>,

    /// WebSocket configuration (for type="websocket").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub websocket: Option<WebSocketProxyConfig>,
}

fn default_operation_type() -> String {
    "app".to_string()
}

impl InlineOperation {
    /// Converts path segments to a URL path string.
    pub fn path_string(&self) -> String {
        format!("/{}", self.path.join("/"))
    }
}

/// Inline subscription definition (from App.subscriptions[]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineSubscription {
    /// Unique identifier for this subscription within the app.
    pub id: String,

    /// Event trigger configuration.
    pub trigger: SubscriptionTrigger,

    /// Webhook channel configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<SubscriptionChannel>,

    /// Notification configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationDef>,
}

/// CustomOperation resource - defines a single API endpoint.
///
/// Can be created:
/// 1. Standalone - with explicit proxy/sql/fhirpath/handler configuration
/// 2. From App.operations[] - with type="app", using App.endpoint for proxying
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomOperation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "resourceType")]
    pub resource_type: String,

    /// Reference to the App resource.
    pub app: Reference,

    /// Path relative to App.basePath (e.g., "/users/:id").
    pub path: String,

    /// HTTP method (GET, POST, PUT, DELETE, PATCH).
    pub method: String,

    /// Operation type: "proxy" | "sql" | "fhirpath" | "handler" | "app".
    #[serde(rename = "type")]
    pub operation_type: String,

    /// Whether this operation is active.
    pub active: bool,

    /// Whether this operation is public (no authentication required).
    #[serde(default)]
    pub public: bool,

    /// Access policy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<OperationPolicy>,

    /// Proxy configuration (for type="proxy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,

    /// SQL query (for type="sql").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,

    /// FHIRPath expression (for type="fhirpath").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhirpath: Option<String>,

    /// Handler name (for type="handler").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,

    /// Whether to include raw request body bytes (base64-encoded) in AppOperationRequest.
    /// Only applicable for type="app" operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_raw_body: Option<bool>,
}

/// AppSubscription resource - defines event subscription for an App.
///
/// Created from App.subscriptions[] and reconciled automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSubscription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "resourceType")]
    pub resource_type: String,

    /// Reference to the App resource.
    pub app: Reference,

    /// Event trigger configuration.
    pub trigger: SubscriptionTrigger,

    /// Webhook channel configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<SubscriptionChannel>,

    /// Notification configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationDef>,

    /// Whether this subscription is active.
    pub active: bool,
}

/// FHIR Reference type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// Proxy configuration for forwarding requests to external services.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Target URL to forward requests to.
    pub url: String,

    /// Request timeout in seconds (default: 30).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,

    /// Whether to forward authentication headers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forward_auth: Option<bool>,

    /// Additional headers to add to the proxied request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,

    /// WebSocket proxy configuration (for type="websocket" operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub websocket: Option<WebSocketProxyConfig>,
}

/// WebSocket proxy configuration for real-time connections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketProxyConfig {
    /// WebSocket URL to connect to (ws:// or wss://).
    pub url: String,

    /// WebSocket subprotocols to negotiate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subprotocols: Option<Vec<String>>,

    /// Whether to forward auth info as query parameters to backend.
    /// When true, adds fhirUser, userId, etc. to the backend WebSocket URL.
    #[serde(default)]
    pub forward_auth_in_query: bool,
}

/// Route key for looking up operations.
/// Format: "{method}:{full_path}"
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
    pub method: String,
    pub path: String,
}

impl RouteKey {
    pub fn new(method: String, path: String) -> Self {
        Self { method, path }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() == 2 {
            Some(Self {
                method: parts[0].to_string(),
                path: parts[1].to_string(),
            })
        } else {
            None
        }
    }
}

impl std::fmt::Display for RouteKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.method, self.path)
    }
}

// =============================================================================
// App Handler Types
// =============================================================================

/// User authentication context for App backends.
///
/// This is a serializable subset of AuthContext containing all auth
/// information that App backends need for authorization decisions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    /// Whether the request is authenticated.
    pub authenticated: bool,

    /// User ID (from UserContext.id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Username for display/logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// FHIR user reference (e.g., "Patient/123" or "Practitioner/456").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// FHIR user resource type (parsed from fhir_user).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user_type: Option<String>,

    /// FHIR user ID only (parsed from fhir_user).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user_id: Option<String>,

    /// User roles.
    #[serde(default)]
    pub roles: Vec<String>,

    /// OAuth scopes (parsed from space-separated string).
    #[serde(default)]
    pub scopes: Vec<String>,

    /// OAuth client ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// SMART launch context: patient reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// SMART launch context: encounter reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,
}

impl AuthInfo {
    /// Converts AuthContext to serializable AuthInfo.
    ///
    /// Extracts all authentication data including:
    /// - User information from UserContext
    /// - OAuth scopes from token claims
    /// - SMART context (patient, encounter)
    /// - Parsed fhir_user components
    pub fn from_auth_context(auth_ctx: Option<&AuthContext>) -> Self {
        let Some(ctx) = auth_ctx else {
            return Self::default();
        };

        // Get fhir_user (prefers UserContext, falls back to token claims)
        let fhir_user = ctx.fhir_user().map(String::from);

        // Parse fhir_user into type and ID
        let (fhir_user_type, fhir_user_id) = fhir_user
            .as_ref()
            .and_then(|fu| Self::parse_fhir_user(fu))
            .unzip();

        Self {
            authenticated: true,
            user_id: ctx.user.as_ref().map(|u| u.id.to_string()),
            username: ctx.user.as_ref().map(|u| u.username.clone()),
            fhir_user,
            fhir_user_type,
            fhir_user_id,
            roles: ctx
                .user
                .as_ref()
                .map(|u| u.roles.clone())
                .unwrap_or_default(),
            scopes: ctx.scopes().map(String::from).collect(),
            client_id: Some(ctx.client_id().to_string()),
            patient: ctx.patient.clone(),
            encounter: ctx.encounter.clone(),
        }
    }

    /// Parses fhir_user reference into (type, id) tuple.
    ///
    /// Handles edge cases:
    /// - "Patient/123" -> Some(("Patient", "123"))
    /// - "Patient/" -> None (empty ID)
    /// - "/123" -> None (empty type)
    /// - "Patient/123/extra" -> Some(("Patient", "123/extra")) (takes first two)
    /// - "Patient" -> None (no slash)
    fn parse_fhir_user(fhir_user: &str) -> Option<(String, String)> {
        let mut parts = fhir_user.splitn(2, '/');
        let resource_type = parts.next()?.trim();
        let id = parts.next()?.trim();

        // Both parts must be non-empty
        if resource_type.is_empty() || id.is_empty() {
            return None;
        }

        Some((resource_type.to_string(), id.to_string()))
    }
}

/// Request sent to App backend for custom operation execution.
///
/// This structure contains all information an App backend needs to:
/// - Identify which operation to execute
/// - Authorize the request
/// - Extract path/query parameters
/// - Process the request body
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppOperationRequest {
    /// Operation ID from the App manifest.
    pub operation_id: String,

    /// Full operation path (e.g., "/psychportal/appointments/book").
    pub operation_path: String,

    /// HTTP method (GET, POST, PUT, DELETE, PATCH).
    pub method: String,

    /// Authentication context.
    pub auth: AuthInfo,

    /// Path parameters extracted from the route (e.g., {:id} -> {"id": "123"}).
    #[serde(default)]
    pub path_params: HashMap<String, String>,

    /// Query parameters from the URL.
    #[serde(default)]
    pub query_params: HashMap<String, String>,

    /// Request body (parsed as JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,

    /// Raw request body bytes (base64-encoded).
    /// Only included if operation.includeRawBody is true.
    /// Useful for binary data, file uploads, or when the app needs original bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_body: Option<String>,

    /// Selected HTTP headers to forward.
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Response from App backend after operation execution.
///
/// The App backend can:
/// - Set HTTP status code
/// - Return arbitrary JSON body
/// - Request FHIR wrapping (Bundle, OperationOutcome)
/// - Set custom response headers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppOperationResponse {
    /// HTTP status code (200, 201, 400, etc.).
    pub status: u16,

    /// Response body as JSON.
    pub body: serde_json::Value,

    /// Optional FHIR wrapper type: "bundle" | "outcome" | "none".
    ///
    /// - "bundle": Wraps body in a FHIR Bundle
    /// - "outcome": Wraps errors in OperationOutcome
    /// - "none" or absent: Returns body as-is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_wrapper: Option<String>,

    /// Custom response headers to set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // AuthInfo::parse_fhir_user tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_fhir_user_valid() {
        assert_eq!(
            AuthInfo::parse_fhir_user("Patient/123"),
            Some(("Patient".to_string(), "123".to_string()))
        );
        assert_eq!(
            AuthInfo::parse_fhir_user("Practitioner/abc-def"),
            Some(("Practitioner".to_string(), "abc-def".to_string()))
        );
        assert_eq!(
            AuthInfo::parse_fhir_user("Organization/org-123"),
            Some(("Organization".to_string(), "org-123".to_string()))
        );
    }

    #[test]
    fn test_parse_fhir_user_with_extra_slashes() {
        // Multiple slashes - should take first two parts
        assert_eq!(
            AuthInfo::parse_fhir_user("Patient/123/version"),
            Some(("Patient".to_string(), "123/version".to_string()))
        );
    }

    #[test]
    fn test_parse_fhir_user_edge_cases() {
        // No slash
        assert_eq!(AuthInfo::parse_fhir_user("Patient"), None);

        // Empty type
        assert_eq!(AuthInfo::parse_fhir_user("/123"), None);

        // Empty ID
        assert_eq!(AuthInfo::parse_fhir_user("Patient/"), None);

        // Empty string
        assert_eq!(AuthInfo::parse_fhir_user(""), None);

        // Only whitespace
        assert_eq!(AuthInfo::parse_fhir_user("  /  "), None);
    }

    #[test]
    fn test_parse_fhir_user_with_whitespace() {
        // Should trim whitespace
        assert_eq!(
            AuthInfo::parse_fhir_user(" Patient / 123 "),
            Some(("Patient".to_string(), "123".to_string()))
        );
    }

    // -------------------------------------------------------------------------
    // AuthInfo default and from_none tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_auth_info_default() {
        let auth = AuthInfo::default();
        assert!(!auth.authenticated);
        assert!(auth.user_id.is_none());
        assert!(auth.username.is_none());
        assert!(auth.fhir_user.is_none());
        assert!(auth.fhir_user_type.is_none());
        assert!(auth.fhir_user_id.is_none());
        assert!(auth.roles.is_empty());
        assert!(auth.scopes.is_empty());
        assert!(auth.client_id.is_none());
        assert!(auth.patient.is_none());
        assert!(auth.encounter.is_none());
    }

    #[test]
    fn test_auth_info_from_none() {
        let auth = AuthInfo::from_auth_context(None);
        assert!(!auth.authenticated);
        assert!(auth.user_id.is_none());
        assert!(auth.roles.is_empty());
        assert!(auth.scopes.is_empty());
    }

    // -------------------------------------------------------------------------
    // AuthInfo serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_auth_info_serialization() {
        let auth = AuthInfo {
            authenticated: true,
            user_id: Some("user-123".to_string()),
            username: Some("testuser".to_string()),
            fhir_user: Some("Patient/456".to_string()),
            fhir_user_type: Some("Patient".to_string()),
            fhir_user_id: Some("456".to_string()),
            roles: vec!["client".to_string()],
            scopes: vec!["patient/*.read".to_string()],
            client_id: Some("my-app".to_string()),
            patient: Some("Patient/456".to_string()),
            encounter: None,
        };

        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["authenticated"], true);
        assert_eq!(json["userId"], "user-123");
        assert_eq!(json["username"], "testuser");
        assert_eq!(json["fhirUser"], "Patient/456");
        assert_eq!(json["fhirUserType"], "Patient");
        assert_eq!(json["fhirUserId"], "456");
        assert_eq!(json["roles"], serde_json::json!(["client"]));
        assert_eq!(json["scopes"], serde_json::json!(["patient/*.read"]));
        assert_eq!(json["clientId"], "my-app");
        assert_eq!(json["patient"], "Patient/456");

        // encounter should not be in JSON (skip_serializing_if None)
        assert!(json.get("encounter").is_none());
    }

    #[test]
    fn test_auth_info_deserialization() {
        let json = serde_json::json!({
            "authenticated": true,
            "userId": "user-123",
            "fhirUserType": "Patient",
            "fhirUserId": "456",
            "roles": ["client"],
            "scopes": ["patient/*.read"]
        });

        let auth: AuthInfo = serde_json::from_value(json).unwrap();
        assert!(auth.authenticated);
        assert_eq!(auth.user_id, Some("user-123".to_string()));
        assert_eq!(auth.fhir_user_type, Some("Patient".to_string()));
        assert_eq!(auth.fhir_user_id, Some("456".to_string()));
        assert_eq!(auth.roles, vec!["client"]);
        assert_eq!(auth.scopes, vec!["patient/*.read"]);
    }

    // -------------------------------------------------------------------------
    // AppOperationRequest serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_app_operation_request_serialization() {
        let mut path_params = HashMap::new();
        path_params.insert("id".to_string(), "123".to_string());

        let mut query_params = HashMap::new();
        query_params.insert("status".to_string(), "active".to_string());

        let req = AppOperationRequest {
            operation_id: "book-appointment".to_string(),
            operation_path: "/psychportal/appointments/book".to_string(),
            method: "POST".to_string(),
            auth: AuthInfo::default(),
            path_params,
            query_params,
            body: Some(serde_json::json!({"date": "2024-01-15"})),
            raw_body: None,
            headers: HashMap::new(),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["operationId"], "book-appointment");
        assert_eq!(json["operationPath"], "/psychportal/appointments/book");
        assert_eq!(json["method"], "POST");
        assert_eq!(json["pathParams"]["id"], "123");
        assert_eq!(json["queryParams"]["status"], "active");
        assert_eq!(json["body"]["date"], "2024-01-15");
    }

    #[test]
    fn test_app_operation_request_deserialization() {
        let json = serde_json::json!({
            "operationId": "get-user",
            "operationPath": "/api/users/123",
            "method": "GET",
            "auth": {
                "authenticated": false
            }
        });

        let req: AppOperationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.operation_id, "get-user");
        assert_eq!(req.method, "GET");
        assert!(!req.auth.authenticated);
        assert!(req.path_params.is_empty());
        assert!(req.query_params.is_empty());
        assert!(req.body.is_none());
    }

    #[test]
    fn test_app_operation_request_with_raw_body() {
        use base64::{Engine, engine::general_purpose::STANDARD};

        // Create request with raw body
        let raw_data = b"binary file data here";
        let encoded = STANDARD.encode(raw_data);

        let req = AppOperationRequest {
            operation_id: "upload-file".to_string(),
            operation_path: "/upload".to_string(),
            method: "POST".to_string(),
            auth: AuthInfo::default(),
            path_params: HashMap::new(),
            query_params: HashMap::new(),
            body: Some(serde_json::json!({"filename": "test.bin"})),
            raw_body: Some(encoded.clone()),
            headers: HashMap::new(),
        };

        // Serialize to JSON
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["rawBody"], encoded);
        assert_eq!(json["body"]["filename"], "test.bin");

        // Deserialize back
        let req2: AppOperationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req2.raw_body, Some(encoded));
        assert_eq!(req2.body.unwrap()["filename"], "test.bin");
    }

    #[test]
    fn test_app_operation_request_without_raw_body() {
        let req = AppOperationRequest {
            operation_id: "create-user".to_string(),
            operation_path: "/users".to_string(),
            method: "POST".to_string(),
            auth: AuthInfo::default(),
            path_params: HashMap::new(),
            query_params: HashMap::new(),
            body: Some(serde_json::json!({"name": "John"})),
            raw_body: None,
            headers: HashMap::new(),
        };

        let json = serde_json::to_value(&req).unwrap();
        // rawBody should not be in JSON when None
        assert!(json.get("rawBody").is_none());
        assert_eq!(json["body"]["name"], "John");
    }

    // -------------------------------------------------------------------------
    // AppOperationResponse serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_app_operation_response_serialization() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "value".to_string());

        let resp = AppOperationResponse {
            status: 201,
            body: serde_json::json!({"id": "Appointment/123"}),
            fhir_wrapper: Some("bundle".to_string()),
            headers: Some(headers),
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], 201);
        assert_eq!(json["body"]["id"], "Appointment/123");
        assert_eq!(json["fhirWrapper"], "bundle");
        assert_eq!(json["headers"]["X-Custom-Header"], "value");
    }

    #[test]
    fn test_app_operation_response_minimal() {
        let resp = AppOperationResponse {
            status: 200,
            body: serde_json::json!({"result": "ok"}),
            fhir_wrapper: None,
            headers: None,
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], 200);
        assert_eq!(json["body"]["result"], "ok");

        // Optional fields should not be present
        assert!(json.get("fhirWrapper").is_none());
        assert!(json.get("headers").is_none());
    }

    #[test]
    fn test_app_operation_response_deserialization() {
        let json = serde_json::json!({
            "status": 400,
            "body": {
                "error": "Invalid request"
            },
            "fhirWrapper": "outcome"
        });

        let resp: AppOperationResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.status, 400);
        assert_eq!(resp.body["error"], "Invalid request");
        assert_eq!(resp.fhir_wrapper, Some("outcome".to_string()));
        assert!(resp.headers.is_none());
    }
}
