//! Gateway resource types (App and CustomOperation).

use serde::{Deserialize, Serialize};

/// App resource - groups custom operations under a common base path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "resourceType")]
    pub resource_type: String,

    /// Human-readable name of the app.
    pub name: String,

    /// Base path for all operations in this app (e.g., "/api/v1/external").
    pub base_path: String,

    /// Whether this app is active.
    pub active: bool,
}

/// CustomOperation resource - defines a single API endpoint.
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

    /// Operation type: "proxy" | "sql" | "fhirpath" | "handler".
    #[serde(rename = "type")]
    pub operation_type: String,

    /// Whether this operation is active.
    pub active: bool,

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
