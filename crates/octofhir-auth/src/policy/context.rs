//! Policy evaluation context for access control decisions.
//!
//! This module provides the context structure that contains all information
//! needed for policy evaluation. The context is serializable to JSON for
//! consumption by script engines (Rhai, QuickJS).
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_auth::policy::context::{PolicyContext, PolicyContextBuilder};
//!
//! let context = PolicyContextBuilder::new()
//!     .with_auth_context(&auth)
//!     .with_request("GET", "/Patient/123", query_params, None)
//!     .with_environment("req-abc", Some(source_ip))
//!     .build()?;
//!
//! // Context can be serialized for script engines
//! let json = serde_json::to_value(&context)?;
//! ```

use std::collections::HashMap;
use std::net::IpAddr;

use octofhir_core::fhir_reference::parse_reference_simple;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::middleware::types::{AuthContext, UserContext};
use crate::smart::scopes::{FhirOperation, ScopeContext, SmartScopes};
use crate::types::Client;

// =============================================================================
// Policy Context
// =============================================================================

/// Complete context for policy evaluation.
///
/// This structure contains all information needed to make access control
/// decisions and is serializable for consumption by script engines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyContext {
    /// User identity information (None for client credentials flow).
    pub user: Option<UserIdentity>,

    /// OAuth client information.
    pub client: ClientIdentity,

    /// Parsed scope information.
    pub scopes: ScopeSummary,

    /// Information about the current request.
    pub request: RequestContext,

    /// Information about the resource being accessed (for read/update/delete).
    pub resource: Option<ResourceContext>,

    /// Environment information.
    pub environment: EnvironmentContext,
}

// =============================================================================
// User Identity
// =============================================================================

/// User identity information for policy evaluation.
///
/// This is a policy-focused view of the user context, with extracted
/// FHIR user type and ID for easier policy rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdentity {
    /// Internal user ID.
    pub id: String,

    /// FHIR user reference (e.g., "Practitioner/123").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// User's FHIR resource type extracted from fhir_user (e.g., "Practitioner").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user_type: Option<String>,

    /// User's FHIR resource ID extracted from fhir_user (e.g., "123").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user_id: Option<String>,

    /// Assigned roles.
    pub roles: Vec<String>,

    /// Custom attributes from IdP or user record.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

impl UserIdentity {
    /// Create a UserIdentity from a UserContext.
    #[must_use]
    pub fn from_user_context(user: &UserContext) -> Self {
        let (fhir_user_type, fhir_user_id) = user
            .fhir_user
            .as_ref()
            .map(|f| parse_fhir_reference(f))
            .unwrap_or((None, None));

        Self {
            id: user.id.to_string(),
            fhir_user: user.fhir_user.clone(),
            fhir_user_type,
            fhir_user_id,
            roles: user.roles.clone(),
            attributes: user.attributes.clone(),
        }
    }

    /// Returns `true` if the user has a specific role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Parse a FHIR reference like "Practitioner/123" into (type, id).
fn parse_fhir_reference(reference: &str) -> (Option<String>, Option<String>) {
    // Use the shared implementation from octofhir-core.
    // Note: We pass None for base_url since fhirUser is typically a simple relative reference.
    match parse_reference_simple(reference, None) {
        Ok((resource_type, id)) => (Some(resource_type), Some(id)),
        Err(_) => (None, None),
    }
}

// =============================================================================
// Client Identity
// =============================================================================

/// OAuth client information for policy evaluation.
///
/// This is a simplified, policy-focused view of the OAuth client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIdentity {
    /// Client ID.
    pub id: String,

    /// Client display name.
    pub name: String,

    /// Whether the client is trusted (confidential with system scopes).
    pub trusted: bool,

    /// Client type classification.
    pub client_type: ClientType,
}

impl ClientIdentity {
    /// Create a ClientIdentity from a Client and scope information.
    #[must_use]
    pub fn from_client(client: &Client, scopes: &SmartScopes) -> Self {
        let client_type = if !client.confidential {
            ClientType::Public
        } else if client.jwks.is_some() || client.jwks_uri.is_some() {
            ClientType::ConfidentialAsymmetric
        } else {
            ClientType::ConfidentialSymmetric
        };

        // A client is trusted if it's confidential and has system-level scopes
        let trusted = client.confidential && scopes.has_system_scopes();

        Self {
            id: client.client_id.clone(),
            name: client.name.clone(),
            trusted,
            client_type,
        }
    }
}

/// OAuth client type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientType {
    /// Public client (browser-based, mobile app).
    Public,
    /// Confidential client with symmetric key (client secret).
    ConfidentialSymmetric,
    /// Confidential client with asymmetric keys (JWKS).
    ConfidentialAsymmetric,
}

// =============================================================================
// Scope Summary
// =============================================================================

/// Serializable summary of parsed SMART scopes.
///
/// This provides a policy-friendly view of the granted scopes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSummary {
    /// Original scope string.
    pub raw: String,

    /// Patient-context scopes (e.g., "patient/Observation.rs").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patient_scopes: Vec<String>,

    /// User-context scopes (e.g., "user/Patient.r").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_scopes: Vec<String>,

    /// System-context scopes (e.g., "system/*.cruds").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_scopes: Vec<String>,

    /// Whether any scope grants wildcard (*) resource access.
    pub has_wildcard: bool,

    /// Whether launch scope is present.
    pub launch: bool,

    /// Whether openid scope is present.
    pub openid: bool,

    /// Whether fhirUser scope is present.
    pub fhir_user: bool,

    /// Whether offline_access scope is present.
    pub offline_access: bool,
}

impl ScopeSummary {
    /// Create a ScopeSummary from a scope string.
    #[must_use]
    pub fn from_scope_string(scope_string: &str) -> Self {
        let scopes = SmartScopes::parse(scope_string).unwrap_or_default();
        Self::from_smart_scopes(scope_string, &scopes)
    }

    /// Create a ScopeSummary from parsed SmartScopes.
    #[must_use]
    pub fn from_smart_scopes(raw: &str, scopes: &SmartScopes) -> Self {
        let mut patient_scopes = Vec::new();
        let mut user_scopes = Vec::new();
        let mut system_scopes = Vec::new();

        for scope in &scopes.resource_scopes {
            let scope_str = scope.to_string();
            match scope.context {
                ScopeContext::Patient => patient_scopes.push(scope_str),
                ScopeContext::User => user_scopes.push(scope_str),
                ScopeContext::System => system_scopes.push(scope_str),
            }
        }

        Self {
            raw: raw.to_string(),
            patient_scopes,
            user_scopes,
            system_scopes,
            has_wildcard: scopes.has_wildcard_access(),
            launch: scopes.launch,
            openid: scopes.openid,
            fhir_user: scopes.fhir_user,
            offline_access: scopes.offline_access,
        }
    }
}

// =============================================================================
// Request Context
// =============================================================================

/// Information about the current FHIR request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    /// Detected FHIR operation type.
    pub operation: FhirOperation,

    /// Operation ID for policy targeting (e.g., "fhir.read", "graphql.query").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,

    /// Resource type being accessed.
    pub resource_type: String,

    /// Resource ID (for instance operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,

    /// Compartment type from URL (e.g., "Patient" in /Patient/123/Observation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compartment_type: Option<String>,

    /// Compartment ID from URL (e.g., "123" in /Patient/123/Observation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compartment_id: Option<String>,

    /// Request body (for create/update).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,

    /// Query parameters.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub query_params: HashMap<String, String>,

    /// Request path.
    pub path: String,

    /// HTTP method.
    pub method: String,
}

impl RequestContext {
    /// Check if the operation is read-only.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        matches!(
            self.operation,
            FhirOperation::Read
                | FhirOperation::VRead
                | FhirOperation::Search
                | FhirOperation::SearchType
                | FhirOperation::SearchSystem
                | FhirOperation::HistoryInstance
                | FhirOperation::HistoryType
                | FhirOperation::HistorySystem
                | FhirOperation::Capabilities
        )
    }
}

// =============================================================================
// Resource Context
// =============================================================================

/// Information about the resource being accessed.
///
/// This is populated for read, update, patch, and delete operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContext {
    /// The actual resource JSON.
    pub resource: serde_json::Value,

    /// Resource ID.
    pub id: String,

    /// Resource type.
    pub resource_type: String,

    /// Resource version ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,

    /// Last updated timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,

    /// Extracted subject reference (Patient, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Extracted author reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

impl ResourceContext {
    /// Create a ResourceContext from a FHIR resource JSON.
    #[must_use]
    pub fn from_resource(resource: serde_json::Value) -> Self {
        let resource_type = resource["resourceType"].as_str().unwrap_or("").to_string();
        let id = resource["id"].as_str().unwrap_or("").to_string();
        let version_id = resource["meta"]["versionId"].as_str().map(String::from);
        let last_updated = resource["meta"]["lastUpdated"].as_str().map(String::from);

        let subject = extract_subject(&resource);
        let author = extract_author(&resource);

        Self {
            resource,
            id,
            resource_type,
            version_id,
            last_updated,
            subject,
            author,
        }
    }
}

/// Extract subject reference from a FHIR resource.
///
/// Looks for common subject fields in order of precedence.
fn extract_subject(resource: &serde_json::Value) -> Option<String> {
    // Try common subject fields in order of specificity
    resource["subject"]["reference"]
        .as_str()
        .or_else(|| resource["patient"]["reference"].as_str())
        .or_else(|| {
            // For Patient resources, the subject is the resource itself
            if resource["resourceType"].as_str() == Some("Patient") {
                resource["id"]
                    .as_str()
                    .map(|id| format!("Patient/{}", id).leak() as &str)
            } else {
                None
            }
        })
        .map(String::from)
}

/// Extract author reference from a FHIR resource.
///
/// Looks for common author fields in order of precedence.
fn extract_author(resource: &serde_json::Value) -> Option<String> {
    resource["author"]["reference"]
        .as_str()
        .or_else(|| resource["author"][0]["reference"].as_str())
        .or_else(|| resource["performer"]["reference"].as_str())
        .or_else(|| resource["performer"][0]["reference"].as_str())
        .or_else(|| resource["performer"][0]["actor"]["reference"].as_str())
        .or_else(|| resource["recorder"]["reference"].as_str())
        .or_else(|| resource["asserter"]["reference"].as_str())
        .or_else(|| resource["requester"]["reference"].as_str())
        .map(String::from)
}

// =============================================================================
// Environment Context
// =============================================================================

/// Environment information for policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentContext {
    /// Current server time (RFC 3339 format for serialization).
    #[serde(with = "time::serde::rfc3339")]
    pub request_time: OffsetDateTime,

    /// Source IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<IpAddr>,

    /// Request ID for tracing.
    pub request_id: String,

    /// SMART patient context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient_context: Option<String>,

    /// SMART encounter context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter_context: Option<String>,
}

// =============================================================================
// FHIR Operation Detection
// =============================================================================

/// Detect the FHIR operation from HTTP method, path, and request characteristics.
///
/// This function parses the request to determine which FHIR interaction is being
/// performed, following the FHIR RESTful API specification.
///
/// # Arguments
///
/// * `method` - HTTP method (GET, POST, PUT, PATCH, DELETE)
/// * `path` - Request path (e.g., "/Patient/123", "/Observation")
/// * `has_body` - Whether the request has a body
///
/// # Examples
///
/// ```
/// use octofhir_auth::policy::context::detect_operation;
/// use octofhir_auth::smart::scopes::FhirOperation;
///
/// assert_eq!(detect_operation("GET", "/Patient/123", false), FhirOperation::Read);
/// assert_eq!(detect_operation("POST", "/Patient", true), FhirOperation::Create);
/// assert_eq!(detect_operation("GET", "/Patient", false), FhirOperation::Search);
/// ```
#[must_use]
pub fn detect_operation(method: &str, path: &str, has_body: bool) -> FhirOperation {
    let path = path.trim_start_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match method.to_uppercase().as_str() {
        "GET" => detect_get_operation(path, &segments),
        "POST" => detect_post_operation(path, &segments, has_body),
        "PUT" => FhirOperation::Update,
        "PATCH" => FhirOperation::Patch,
        "DELETE" => FhirOperation::Delete,
        _ => FhirOperation::Read, // Default for unknown methods
    }
}

fn detect_get_operation(path: &str, segments: &[&str]) -> FhirOperation {
    // Check for metadata/capabilities
    if path == "metadata" || path.is_empty() && path.contains("metadata") {
        return FhirOperation::Capabilities;
    }

    // Check for history operations
    if path.contains("/_history") {
        if segments.len() >= 4 && segments[2] == "_history" {
            // /ResourceType/id/_history/vid -> VRead
            return FhirOperation::VRead;
        } else if segments.len() == 3 && segments[2] == "_history" {
            // /ResourceType/id/_history -> HistoryInstance
            return FhirOperation::HistoryInstance;
        } else if segments.len() == 2 && segments[1] == "_history" {
            // /ResourceType/_history -> HistoryType
            return FhirOperation::HistoryType;
        } else if segments.len() == 1 && segments[0] == "_history" {
            // /_history -> HistorySystem
            return FhirOperation::HistorySystem;
        }
    }

    // Check path structure
    match segments.len() {
        0 => FhirOperation::SearchSystem,
        1 => {
            // /ResourceType -> Search (type-level)
            if segments[0].starts_with('$') {
                FhirOperation::Operation
            } else {
                FhirOperation::Search
            }
        }
        2 => {
            // /ResourceType/id -> Read
            // /ResourceType/$operation -> Operation
            if segments[1].starts_with('$') {
                FhirOperation::Operation
            } else if segments[1].starts_with('_') {
                // /_search or other special
                FhirOperation::Search
            } else {
                FhirOperation::Read
            }
        }
        3 => {
            // /Patient/123/Observation -> Search (compartment)
            // /ResourceType/id/$operation -> Operation
            if segments[2].starts_with('$') {
                FhirOperation::Operation
            } else {
                FhirOperation::Search
            }
        }
        _ => FhirOperation::Search,
    }
}

fn detect_post_operation(path: &str, segments: &[&str], has_body: bool) -> FhirOperation {
    // Check for _search
    if path.contains("/_search") {
        return if segments.len() <= 2 {
            FhirOperation::SearchType
        } else {
            FhirOperation::SearchSystem
        };
    }

    // Check for operations
    if path.contains("/$") || (segments.len() == 1 && segments[0].starts_with('$')) {
        return FhirOperation::Operation;
    }

    // Check for batch/transaction at root
    if segments.is_empty() && has_body {
        // This could be batch or transaction - we'd need to look at the body
        // Default to Batch, the caller can refine based on Bundle.type
        return FhirOperation::Batch;
    }

    // POST to /ResourceType with body -> Create
    if segments.len() == 1 && has_body {
        return FhirOperation::Create;
    }

    // Default to search for POST without clear intent
    FhirOperation::Search
}

// =============================================================================
// Path Parsing
// =============================================================================

/// Parse a FHIR path to extract resource type, ID, and compartment information.
///
/// # Returns
///
/// A tuple of (resource_type, resource_id, compartment_type, compartment_id).
#[must_use]
pub fn parse_fhir_path(path: &str) -> (String, Option<String>, Option<String>, Option<String>) {
    let path = path.trim_start_matches('/');
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('?'))
        .collect();

    match segments.as_slice() {
        // Empty
        [] => ("".to_string(), None, None, None),

        // /ResourceType
        [resource_type] => (resource_type.to_string(), None, None, None),

        // /ResourceType/id (not special paths)
        [resource_type, id] if !id.starts_with('_') && !id.starts_with('$') => {
            (resource_type.to_string(), Some(id.to_string()), None, None)
        }

        // /ResourceType/_search or /ResourceType/$operation
        [resource_type, _special] => (resource_type.to_string(), None, None, None),

        // /ResourceType/id/_history
        [resource_type, id, "_history"] if !id.starts_with('_') && !id.starts_with('$') => {
            (resource_type.to_string(), Some(id.to_string()), None, None)
        }

        // /ResourceType/id/_history/vid
        [resource_type, id, "_history", _vid] if !id.starts_with('_') && !id.starts_with('$') => {
            (resource_type.to_string(), Some(id.to_string()), None, None)
        }

        // /CompartmentType/compartmentId/ResourceType (compartment search)
        // e.g., /Patient/123/Observation
        [compartment_type, compartment_id, resource_type]
            if !compartment_id.starts_with('_')
                && !compartment_id.starts_with('$')
                && !resource_type.starts_with('_')
                && !resource_type.starts_with('$') =>
        {
            (
                resource_type.to_string(),
                None,
                Some(compartment_type.to_string()),
                Some(compartment_id.to_string()),
            )
        }

        // /ResourceType/id/$operation or /ResourceType/id/_something
        [resource_type, id, _special] if !id.starts_with('_') && !id.starts_with('$') => {
            (resource_type.to_string(), Some(id.to_string()), None, None)
        }

        // Fallback: first segment is resource type
        _ => (segments[0].to_string(), None, None, None),
    }
}

// =============================================================================
// Policy Context Builder
// =============================================================================

/// Builder for constructing a PolicyContext.
///
/// The builder validates that all required fields are provided before
/// constructing the final context.
#[derive(Debug, Default)]
pub struct PolicyContextBuilder {
    user: Option<UserIdentity>,
    client: Option<ClientIdentity>,
    scopes: Option<ScopeSummary>,
    request: Option<RequestContext>,
    resource: Option<ResourceContext>,
    environment: Option<EnvironmentContext>,
}

impl PolicyContextBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set identity information from an AuthContext.
    #[must_use]
    pub fn with_auth_context(mut self, auth: &AuthContext) -> Self {
        // Extract user identity
        self.user = auth.user.as_ref().map(UserIdentity::from_user_context);

        // Parse scopes
        let scope_string = &auth.token_claims.scope;
        let scopes = SmartScopes::parse(scope_string).unwrap_or_default();

        // Extract client identity
        self.client = Some(ClientIdentity::from_client(&auth.client, &scopes));
        self.scopes = Some(ScopeSummary::from_smart_scopes(scope_string, &scopes));

        self
    }

    /// Set request information.
    #[must_use]
    pub fn with_request(
        mut self,
        method: &str,
        path: &str,
        query_params: HashMap<String, String>,
        body: Option<serde_json::Value>,
    ) -> Self {
        let operation = detect_operation(method, path, body.is_some());
        let (resource_type, resource_id, compartment_type, compartment_id) = parse_fhir_path(path);

        self.request = Some(RequestContext {
            operation,
            operation_id: None,
            resource_type,
            resource_id,
            compartment_type,
            compartment_id,
            body,
            query_params,
            path: path.to_string(),
            method: method.to_string(),
        });
        self
    }

    /// Set the operation ID for policy targeting.
    #[must_use]
    pub fn with_operation_id(mut self, operation_id: impl Into<String>) -> Self {
        if let Some(ref mut request) = self.request {
            request.operation_id = Some(operation_id.into());
        }
        self
    }

    /// Set the existing resource (for read/update/delete operations).
    #[must_use]
    pub fn with_resource(mut self, resource: serde_json::Value) -> Self {
        self.resource = Some(ResourceContext::from_resource(resource));
        self
    }

    /// Set environment information.
    #[must_use]
    pub fn with_environment(mut self, request_id: String, source_ip: Option<IpAddr>) -> Self {
        self.environment = Some(EnvironmentContext {
            request_time: OffsetDateTime::now_utc(),
            source_ip,
            request_id,
            patient_context: None,
            encounter_context: None,
        });
        self
    }

    /// Set SMART launch context.
    #[must_use]
    pub fn with_launch_context(
        mut self,
        patient: Option<String>,
        encounter: Option<String>,
    ) -> Self {
        if let Some(ref mut env) = self.environment {
            env.patient_context = patient;
            env.encounter_context = encounter;
        }
        self
    }

    /// Build the PolicyContext.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<PolicyContext, ContextError> {
        Ok(PolicyContext {
            user: self.user,
            client: self.client.ok_or(ContextError::MissingClient)?,
            scopes: self.scopes.ok_or(ContextError::MissingScopes)?,
            request: self.request.ok_or(ContextError::MissingRequest)?,
            resource: self.resource,
            environment: self.environment.ok_or(ContextError::MissingEnvironment)?,
        })
    }
}

// =============================================================================
// Errors
// =============================================================================

/// Errors that can occur when building a PolicyContext.
///
/// This enum uses the "Missing*" naming pattern which is appropriate for
/// builder validation errors, hence the allow attribute.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::enum_variant_names)]
pub enum ContextError {
    /// Client identity not provided.
    #[error("Missing client identity")]
    MissingClient,

    /// Scope information not provided.
    #[error("Missing scope information")]
    MissingScopes,

    /// Request context not provided.
    #[error("Missing request context")]
    MissingRequest,

    /// Environment context not provided.
    #[error("Missing environment context")]
    MissingEnvironment,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::jwt::AccessTokenClaims;
    use crate::types::GrantType;
    use uuid::Uuid;

    // -------------------------------------------------------------------------
    // Test Helpers
    // -------------------------------------------------------------------------

    fn create_test_claims() -> AccessTokenClaims {
        AccessTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user123".to_string(),
            aud: vec!["https://fhir.example.com".to_string()],
            exp: 9999999999,
            iat: 1000000000,
            jti: "test-jti-123".to_string(),
            scope: "openid patient/Patient.read patient/Observation.rs".to_string(),
            client_id: "test-client".to_string(),
            patient: Some("Patient/123".to_string()),
            encounter: None,
            fhir_user: Some("Practitioner/456".to_string()),
        }
    }

    fn create_test_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
            scopes: vec![],
            confidential: false,
            active: true,
            access_token_lifetime: None,
            refresh_token_lifetime: None,
            pkce_required: None,
            allowed_origins: vec![],
            jwks: None,
            jwks_uri: None,
        }
    }

    fn create_test_user_context() -> UserContext {
        UserContext {
            id: Uuid::new_v4(),
            username: "testuser".to_string(),
            fhir_user: Some("Practitioner/789".to_string()),
            roles: vec!["practitioner".to_string(), "admin".to_string()],
            attributes: HashMap::new(),
        }
    }

    fn create_test_auth_context() -> AuthContext {
        AuthContext {
            token_claims: create_test_claims(),
            client: create_test_client(),
            user: Some(create_test_user_context()),
            patient: Some("Patient/123".to_string()),
            encounter: None,
        }
    }

    // -------------------------------------------------------------------------
    // FHIR Operation Detection Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_detect_operation_read() {
        assert_eq!(
            detect_operation("GET", "/Patient/123", false),
            FhirOperation::Read
        );
        assert_eq!(
            detect_operation("GET", "Patient/123", false),
            FhirOperation::Read
        );
    }

    #[test]
    fn test_detect_operation_create() {
        assert_eq!(
            detect_operation("POST", "/Patient", true),
            FhirOperation::Create
        );
    }

    #[test]
    fn test_detect_operation_update() {
        assert_eq!(
            detect_operation("PUT", "/Patient/123", true),
            FhirOperation::Update
        );
    }

    #[test]
    fn test_detect_operation_patch() {
        assert_eq!(
            detect_operation("PATCH", "/Patient/123", true),
            FhirOperation::Patch
        );
    }

    #[test]
    fn test_detect_operation_delete() {
        assert_eq!(
            detect_operation("DELETE", "/Patient/123", false),
            FhirOperation::Delete
        );
    }

    #[test]
    fn test_detect_operation_search() {
        assert_eq!(
            detect_operation("GET", "/Patient", false),
            FhirOperation::Search
        );
        assert_eq!(
            detect_operation("GET", "/Patient?name=john", false),
            FhirOperation::Search
        );
    }

    #[test]
    fn test_detect_operation_search_type() {
        assert_eq!(
            detect_operation("POST", "/Patient/_search", true),
            FhirOperation::SearchType
        );
    }

    #[test]
    fn test_detect_operation_vread() {
        assert_eq!(
            detect_operation("GET", "/Patient/123/_history/1", false),
            FhirOperation::VRead
        );
    }

    #[test]
    fn test_detect_operation_history_instance() {
        assert_eq!(
            detect_operation("GET", "/Patient/123/_history", false),
            FhirOperation::HistoryInstance
        );
    }

    #[test]
    fn test_detect_operation_history_type() {
        assert_eq!(
            detect_operation("GET", "/Patient/_history", false),
            FhirOperation::HistoryType
        );
    }

    #[test]
    fn test_detect_operation_capabilities() {
        assert_eq!(
            detect_operation("GET", "/metadata", false),
            FhirOperation::Capabilities
        );
    }

    #[test]
    fn test_detect_operation_operation() {
        assert_eq!(
            detect_operation("POST", "/Patient/$validate", true),
            FhirOperation::Operation
        );
        assert_eq!(
            detect_operation("GET", "/Patient/123/$everything", false),
            FhirOperation::Operation
        );
    }

    #[test]
    fn test_detect_operation_compartment_search() {
        assert_eq!(
            detect_operation("GET", "/Patient/123/Observation", false),
            FhirOperation::Search
        );
    }

    // -------------------------------------------------------------------------
    // Path Parsing Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_fhir_path_resource_type() {
        let (rt, id, ct, cid) = parse_fhir_path("/Patient");
        assert_eq!(rt, "Patient");
        assert!(id.is_none());
        assert!(ct.is_none());
        assert!(cid.is_none());
    }

    #[test]
    fn test_parse_fhir_path_resource_instance() {
        let (rt, id, ct, cid) = parse_fhir_path("/Patient/123");
        assert_eq!(rt, "Patient");
        assert_eq!(id, Some("123".to_string()));
        assert!(ct.is_none());
        assert!(cid.is_none());
    }

    #[test]
    fn test_parse_fhir_path_compartment() {
        let (rt, id, ct, cid) = parse_fhir_path("/Patient/123/Observation");
        assert_eq!(rt, "Observation");
        assert!(id.is_none());
        assert_eq!(ct, Some("Patient".to_string()));
        assert_eq!(cid, Some("123".to_string()));
    }

    #[test]
    fn test_parse_fhir_path_history() {
        let (rt, id, _, _) = parse_fhir_path("/Patient/123/_history");
        assert_eq!(rt, "Patient");
        assert_eq!(id, Some("123".to_string()));
    }

    #[test]
    fn test_parse_fhir_path_vread() {
        let (rt, id, _, _) = parse_fhir_path("/Patient/123/_history/1");
        assert_eq!(rt, "Patient");
        assert_eq!(id, Some("123".to_string()));
    }

    // -------------------------------------------------------------------------
    // User Identity Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_user_identity_from_context() {
        let user_ctx = create_test_user_context();
        let user = UserIdentity::from_user_context(&user_ctx);

        assert_eq!(user.fhir_user, Some("Practitioner/789".to_string()));
        assert_eq!(user.fhir_user_type, Some("Practitioner".to_string()));
        assert_eq!(user.fhir_user_id, Some("789".to_string()));
        assert!(user.has_role("practitioner"));
        assert!(user.has_role("admin"));
        assert!(!user.has_role("patient"));
    }

    #[test]
    fn test_parse_fhir_reference() {
        let (rtype, rid) = parse_fhir_reference("Practitioner/123");
        assert_eq!(rtype, Some("Practitioner".to_string()));
        assert_eq!(rid, Some("123".to_string()));

        let (rtype, rid) = parse_fhir_reference("invalid");
        assert!(rtype.is_none());
        assert!(rid.is_none());
    }

    // -------------------------------------------------------------------------
    // Client Identity Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_client_identity_public() {
        let client = create_test_client();
        let scopes = SmartScopes::parse("patient/Patient.r").unwrap();
        let identity = ClientIdentity::from_client(&client, &scopes);

        assert_eq!(identity.client_type, ClientType::Public);
        assert!(!identity.trusted);
    }

    #[test]
    fn test_client_identity_confidential_symmetric() {
        let mut client = create_test_client();
        client.confidential = true;
        client.client_secret = Some("secret".to_string());

        let scopes = SmartScopes::parse("system/*.cruds").unwrap();
        let identity = ClientIdentity::from_client(&client, &scopes);

        assert_eq!(identity.client_type, ClientType::ConfidentialSymmetric);
        assert!(identity.trusted); // confidential + system scopes
    }

    // -------------------------------------------------------------------------
    // Scope Summary Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_scope_summary() {
        let summary = ScopeSummary::from_scope_string(
            "launch openid patient/Observation.rs user/Patient.r system/*.cruds",
        );

        assert!(summary.launch);
        assert!(summary.openid);
        assert_eq!(summary.patient_scopes.len(), 1);
        assert_eq!(summary.user_scopes.len(), 1);
        assert_eq!(summary.system_scopes.len(), 1);
        assert!(summary.has_wildcard);
    }

    // -------------------------------------------------------------------------
    // Resource Context Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_resource_context_observation() {
        let resource = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-123",
            "meta": {
                "versionId": "1",
                "lastUpdated": "2024-01-15T10:30:00Z"
            },
            "subject": {
                "reference": "Patient/456"
            },
            "performer": [{
                "reference": "Practitioner/789"
            }]
        });

        let ctx = ResourceContext::from_resource(resource);

        assert_eq!(ctx.resource_type, "Observation");
        assert_eq!(ctx.id, "obs-123");
        assert_eq!(ctx.version_id, Some("1".to_string()));
        assert_eq!(ctx.subject, Some("Patient/456".to_string()));
        assert_eq!(ctx.author, Some("Practitioner/789".to_string()));
    }

    #[test]
    fn test_resource_context_patient() {
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "id": "pat-123",
            "name": [{"family": "Smith"}]
        });

        let ctx = ResourceContext::from_resource(resource);

        assert_eq!(ctx.resource_type, "Patient");
        assert_eq!(ctx.id, "pat-123");
        // Patient resources: subject is self
        assert_eq!(ctx.subject, Some("Patient/pat-123".to_string()));
    }

    // -------------------------------------------------------------------------
    // Policy Context Builder Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_context_builder() {
        let auth = create_test_auth_context();
        let context = PolicyContextBuilder::new()
            .with_auth_context(&auth)
            .with_request("GET", "/Patient/123", HashMap::new(), None)
            .with_environment("req-123".to_string(), None)
            .build()
            .unwrap();

        assert!(context.user.is_some());
        assert_eq!(context.client.id, "test-client");
        assert_eq!(context.request.resource_type, "Patient");
        assert_eq!(context.request.resource_id, Some("123".to_string()));
        assert_eq!(context.request.operation, FhirOperation::Read);
        assert!(context.request.is_read_only());
    }

    #[test]
    fn test_context_builder_with_resource() {
        let auth = create_test_auth_context();
        let resource = serde_json::json!({
            "resourceType": "Observation",
            "id": "obs-456",
            "subject": { "reference": "Patient/123" }
        });

        let context = PolicyContextBuilder::new()
            .with_auth_context(&auth)
            .with_request("GET", "/Observation/456", HashMap::new(), None)
            .with_resource(resource)
            .with_environment("req-123".to_string(), None)
            .build()
            .unwrap();

        assert!(context.resource.is_some());
        let res = context.resource.unwrap();
        assert_eq!(res.subject, Some("Patient/123".to_string()));
    }

    #[test]
    fn test_context_builder_with_launch_context() {
        let auth = create_test_auth_context();
        let context = PolicyContextBuilder::new()
            .with_auth_context(&auth)
            .with_request("GET", "/Patient/123", HashMap::new(), None)
            .with_environment("req-123".to_string(), None)
            .with_launch_context(Some("Patient/123".to_string()), None)
            .build()
            .unwrap();

        assert_eq!(
            context.environment.patient_context,
            Some("Patient/123".to_string())
        );
    }

    #[test]
    fn test_context_builder_missing_client() {
        let result = PolicyContextBuilder::new()
            .with_request("GET", "/Patient/123", HashMap::new(), None)
            .with_environment("req-123".to_string(), None)
            .build();

        assert!(matches!(result, Err(ContextError::MissingClient)));
    }

    // -------------------------------------------------------------------------
    // Read-Only Operation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_read_only() {
        let auth = create_test_auth_context();

        // Read operations
        for (method, path) in [
            ("GET", "/Patient/123"),
            ("GET", "/Patient"),
            ("GET", "/Patient/123/_history"),
            ("GET", "/metadata"),
        ] {
            let context = PolicyContextBuilder::new()
                .with_auth_context(&auth)
                .with_request(method, path, HashMap::new(), None)
                .with_environment("req-123".to_string(), None)
                .build()
                .unwrap();
            assert!(
                context.request.is_read_only(),
                "{} {} should be read-only",
                method,
                path
            );
        }

        // Write operations
        for (method, path, has_body) in [
            ("POST", "/Patient", true),
            ("PUT", "/Patient/123", true),
            ("PATCH", "/Patient/123", true),
            ("DELETE", "/Patient/123", false),
        ] {
            let body = if has_body {
                Some(serde_json::json!({}))
            } else {
                None
            };
            let context = PolicyContextBuilder::new()
                .with_auth_context(&auth)
                .with_request(method, path, HashMap::new(), body)
                .with_environment("req-123".to_string(), None)
                .build()
                .unwrap();
            assert!(
                !context.request.is_read_only(),
                "{} {} should not be read-only",
                method,
                path
            );
        }
    }

    // -------------------------------------------------------------------------
    // JSON Serialization Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_context_serialization() {
        let auth = create_test_auth_context();
        let context = PolicyContextBuilder::new()
            .with_auth_context(&auth)
            .with_request("GET", "/Patient/123", HashMap::new(), None)
            .with_environment("req-123".to_string(), None)
            .build()
            .unwrap();

        // Should serialize to JSON without errors
        let json = serde_json::to_value(&context).unwrap();

        // Check camelCase conversion
        assert!(json.get("resourceType").is_none()); // Not at top level
        assert!(json.get("request").is_some());
        assert!(json["request"].get("resourceType").is_some());
    }
}
