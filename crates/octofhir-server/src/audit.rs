//! Audit Trail Service for creating FHIR AuditEvent resources.
//!
//! This module provides audit logging functionality that creates standard FHIR R4
//! AuditEvent resources for tracking system activity, including:
//!
//! - FHIR REST operations (create, read, update, delete, search)
//! - Authentication events (login, logout, failed attempts)
//! - Admin operations (client/user management, config changes)
//!
//! The AuditEvent resources are stored via the standard FHIR storage and can be
//! queried using the normal FHIR search API.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::IpAddr;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::config::AuditConfig;
use octofhir_storage::DynStorage;

/// Audit action types that map to AuditEvent.subtype
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    // User authentication events
    UserLogin,
    UserLogout,
    UserLoginFailed,

    // FHIR resource operations
    ResourceCreate,
    ResourceRead,
    ResourceUpdate,
    ResourceDelete,
    ResourceSearch,

    // Policy evaluation
    PolicyEvaluate,

    // Client management
    ClientAuth,
    ClientCreate,
    ClientUpdate,
    ClientDelete,

    // Configuration changes
    ConfigChange,

    // System events
    SystemStartup,
    SystemShutdown,
}

impl AuditAction {
    /// Returns the FHIR action code (C, R, U, D, E)
    pub fn to_action_code(&self) -> &'static str {
        match self {
            AuditAction::ResourceCreate | AuditAction::ClientCreate => "C",
            AuditAction::ResourceRead | AuditAction::ResourceSearch => "R",
            AuditAction::ResourceUpdate | AuditAction::ClientUpdate | AuditAction::ConfigChange => {
                "U"
            }
            AuditAction::ResourceDelete | AuditAction::ClientDelete => "D",
            _ => "E", // Execute for all other actions
        }
    }

    /// Returns the subtype code for the AuditEvent
    pub fn to_subtype_code(&self) -> &'static str {
        match self {
            AuditAction::UserLogin => "user.login",
            AuditAction::UserLogout => "user.logout",
            AuditAction::UserLoginFailed => "user.login_failed",
            AuditAction::ResourceCreate => "resource.create",
            AuditAction::ResourceRead => "resource.read",
            AuditAction::ResourceUpdate => "resource.update",
            AuditAction::ResourceDelete => "resource.delete",
            AuditAction::ResourceSearch => "resource.search",
            AuditAction::PolicyEvaluate => "policy.evaluate",
            AuditAction::ClientAuth => "client.auth",
            AuditAction::ClientCreate => "client.create",
            AuditAction::ClientUpdate => "client.update",
            AuditAction::ClientDelete => "client.delete",
            AuditAction::ConfigChange => "config.change",
            AuditAction::SystemStartup => "system.startup",
            AuditAction::SystemShutdown => "system.shutdown",
        }
    }

    /// Returns a human-readable display name
    pub fn display(&self) -> &'static str {
        match self {
            AuditAction::UserLogin => "User Login",
            AuditAction::UserLogout => "User Logout",
            AuditAction::UserLoginFailed => "Login Failed",
            AuditAction::ResourceCreate => "Resource Created",
            AuditAction::ResourceRead => "Resource Read",
            AuditAction::ResourceUpdate => "Resource Updated",
            AuditAction::ResourceDelete => "Resource Deleted",
            AuditAction::ResourceSearch => "Resource Search",
            AuditAction::PolicyEvaluate => "Policy Evaluated",
            AuditAction::ClientAuth => "Client Authentication",
            AuditAction::ClientCreate => "Client Created",
            AuditAction::ClientUpdate => "Client Updated",
            AuditAction::ClientDelete => "Client Deleted",
            AuditAction::ConfigChange => "Configuration Changed",
            AuditAction::SystemStartup => "System Started",
            AuditAction::SystemShutdown => "System Stopped",
        }
    }
}

/// Audit outcome - maps to FHIR AuditEvent.outcome codes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    /// Success (outcome code 0)
    Success,
    /// Minor failure (outcome code 4)
    MinorFailure,
    /// Serious failure (outcome code 8)
    SeriousFailure,
    /// Major failure (outcome code 12)
    MajorFailure,
}

impl AuditOutcome {
    /// Returns the FHIR outcome code
    pub fn to_code(&self) -> &'static str {
        match self {
            AuditOutcome::Success => "0",
            AuditOutcome::MinorFailure => "4",
            AuditOutcome::SeriousFailure => "8",
            AuditOutcome::MajorFailure => "12",
        }
    }

    /// Returns a human-readable display name
    pub fn display(&self) -> &'static str {
        match self {
            AuditOutcome::Success => "Success",
            AuditOutcome::MinorFailure => "Minor Failure",
            AuditOutcome::SeriousFailure => "Serious Failure",
            AuditOutcome::MajorFailure => "Major Failure",
        }
    }
}

/// Actor type for the audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorType {
    User {
        id: String,
        name: Option<String>,
        fhir_user: Option<String>,
    },
    Client {
        id: String,
        name: Option<String>,
    },
    System,
}

/// Source information for the audit event
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditSource {
    pub ip_address: Option<IpAddr>,
    pub user_agent: Option<String>,
    pub site: Option<String>,
}

/// Target entity for the audit event (the resource being acted upon)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditEntity {
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub query: Option<String>,
}

/// Context for the audit event
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditContext {
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub client_id: Option<String>,
}

/// Builder for creating audit events
#[derive(Debug, Clone)]
pub struct AuditEventBuilder {
    action: AuditAction,
    outcome: AuditOutcome,
    outcome_desc: Option<String>,
    actor: Option<ActorType>,
    source: AuditSource,
    entity: Option<AuditEntity>,
    context: AuditContext,
}

impl AuditEventBuilder {
    /// Create a new audit event builder
    pub fn new(action: AuditAction) -> Self {
        Self {
            action,
            outcome: AuditOutcome::Success,
            outcome_desc: None,
            actor: None,
            source: AuditSource::default(),
            entity: None,
            context: AuditContext::default(),
        }
    }

    /// Set the outcome
    pub fn outcome(mut self, outcome: AuditOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Set outcome description (for failures)
    pub fn outcome_desc(mut self, desc: impl Into<String>) -> Self {
        self.outcome_desc = Some(desc.into());
        self
    }

    /// Set the actor as a user
    pub fn user(
        mut self,
        id: impl Into<String>,
        name: Option<String>,
        fhir_user: Option<String>,
    ) -> Self {
        self.actor = Some(ActorType::User {
            id: id.into(),
            name,
            fhir_user,
        });
        self
    }

    /// Set the actor as a client
    pub fn client(mut self, id: impl Into<String>, name: Option<String>) -> Self {
        self.actor = Some(ActorType::Client {
            id: id.into(),
            name,
        });
        self
    }

    /// Set the actor as system
    pub fn system(mut self) -> Self {
        self.actor = Some(ActorType::System);
        self
    }

    /// Set the source IP address
    pub fn ip_address(mut self, ip: IpAddr) -> Self {
        self.source.ip_address = Some(ip);
        self
    }

    /// Set the user agent
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.source.user_agent = Some(ua.into());
        self
    }

    /// Set the source site
    pub fn site(mut self, site: impl Into<String>) -> Self {
        self.source.site = Some(site.into());
        self
    }

    /// Set the target entity
    pub fn entity(
        mut self,
        resource_type: Option<String>,
        resource_id: Option<String>,
        query: Option<String>,
    ) -> Self {
        self.entity = Some(AuditEntity {
            resource_type,
            resource_id,
            query,
        });
        self
    }

    /// Set the request ID
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.context.request_id = Some(id.into());
        self
    }

    /// Set the session ID
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.context.session_id = Some(id.into());
        self
    }

    /// Set the client ID
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.context.client_id = Some(id.into());
        self
    }

    /// Build the FHIR AuditEvent resource
    pub fn build(self) -> Value {
        let now = OffsetDateTime::now_utc();
        let id = Uuid::new_v4().to_string();

        // Build the agent (actor) array
        let mut agents = Vec::new();

        match &self.actor {
            Some(ActorType::User {
                id,
                name,
                fhir_user,
            }) => {
                let mut agent = json!({
                    "type": {
                        "coding": [{
                            "system": "http://dicom.nema.org/resources/ontology/DCM",
                            "code": "110153",
                            "display": "Source Role ID"
                        }]
                    },
                    "who": {
                        "identifier": {
                            "value": id.to_string()
                        }
                    },
                    "requestor": true
                });

                if let Some(name) = name {
                    agent["who"]["display"] = json!(name);
                }
                if let Some(fhir_user) = fhir_user {
                    agent["who"]["reference"] = json!(fhir_user);
                }

                agents.push(agent);
            }
            Some(ActorType::Client { id, name }) => {
                let mut agent = json!({
                    "type": {
                        "coding": [{
                            "system": "http://dicom.nema.org/resources/ontology/DCM",
                            "code": "110150",
                            "display": "Application"
                        }]
                    },
                    "who": {
                        "identifier": {
                            "value": id
                        }
                    },
                    "requestor": true
                });

                if let Some(name) = name {
                    agent["who"]["display"] = json!(name);
                }

                agents.push(agent);
            }
            Some(ActorType::System) | None => {
                agents.push(json!({
                    "type": {
                        "coding": [{
                            "system": "http://dicom.nema.org/resources/ontology/DCM",
                            "code": "110150",
                            "display": "Application"
                        }]
                    },
                    "who": {
                        "display": "OctoFHIR Server"
                    },
                    "requestor": false
                }));
            }
        }

        // Build source
        let mut source = json!({
            "observer": {
                "display": "OctoFHIR Server"
            }
        });

        if let Some(site) = &self.source.site {
            source["site"] = json!(site);
        }

        // Build entity array if we have an entity
        let entities: Vec<Value> = if let Some(entity) = &self.entity {
            let mut e = json!({
                "type": {
                    "system": "http://terminology.hl7.org/CodeSystem/audit-entity-type",
                    "code": "2",
                    "display": "System Object"
                }
            });

            if let Some(rt) = &entity.resource_type {
                if let Some(rid) = &entity.resource_id {
                    e["what"] = json!({
                        "reference": format!("{}/{}", rt, rid)
                    });
                } else {
                    e["what"] = json!({
                        "type": rt
                    });
                }
            }

            if let Some(query) = &entity.query {
                // Store query as description since query field in FHIR is base64-encoded
                // For simplicity, we include it in the entity description
                e["description"] = json!(query);
            }

            vec![e]
        } else {
            vec![]
        };

        // Build the AuditEvent
        let mut event = json!({
            "resourceType": "AuditEvent",
            "id": id,
            "type": {
                "system": "http://dicom.nema.org/resources/ontology/DCM",
                "code": "110100",
                "display": "Application Activity"
            },
            "subtype": [{
                "system": "http://octofhir.io/CodeSystem/audit-action",
                "code": self.action.to_subtype_code(),
                "display": self.action.display()
            }],
            "action": self.action.to_action_code(),
            "recorded": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "outcome": self.outcome.to_code(),
            "agent": agents,
            "source": source
        });

        // Add outcome description if present
        if let Some(desc) = &self.outcome_desc {
            event["outcomeDesc"] = json!(desc);
        }

        // Add entities if present
        if !entities.is_empty() {
            event["entity"] = json!(entities);
        }

        // Add extension for source details (IP, user agent)
        let mut extensions = Vec::new();

        if let Some(ip) = &self.source.ip_address {
            extensions.push(json!({
                "url": "http://octofhir.io/StructureDefinition/audit-source-ip",
                "valueString": ip.to_string()
            }));
        }

        if let Some(ua) = &self.source.user_agent {
            extensions.push(json!({
                "url": "http://octofhir.io/StructureDefinition/audit-user-agent",
                "valueString": ua
            }));
        }

        if let Some(req_id) = &self.context.request_id {
            extensions.push(json!({
                "url": "http://octofhir.io/StructureDefinition/audit-request-id",
                "valueString": req_id
            }));
        }

        if let Some(session_id) = &self.context.session_id {
            extensions.push(json!({
                "url": "http://octofhir.io/StructureDefinition/audit-session-id",
                "valueString": session_id
            }));
        }

        if let Some(client_id) = &self.context.client_id {
            extensions.push(json!({
                "url": "http://octofhir.io/StructureDefinition/audit-client-id",
                "valueString": client_id
            }));
        }

        if !extensions.is_empty() {
            event["extension"] = json!(extensions);
        }

        event
    }
}

/// Audit service for creating and storing audit events
#[derive(Clone)]
pub struct AuditService {
    storage: DynStorage,
    config: AuditConfig,
}

impl AuditService {
    /// Create a new audit service
    pub fn new(storage: DynStorage, _enabled: bool, config: AuditConfig) -> Self {
        Self { storage, config }
    }

    /// Check if audit logging is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if a specific action should be logged based on config
    pub fn should_log(&self, action: &AuditAction, resource_type: Option<&str>) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Check excluded resource types
        if let Some(rt) = resource_type
            && self.config.exclude_resource_types.iter().any(|e| e == rt)
        {
            return false;
        }

        match action {
            // Auth events
            AuditAction::UserLogin
            | AuditAction::UserLogout
            | AuditAction::UserLoginFailed
            | AuditAction::ClientAuth => self.config.log_auth_events,

            // Read operations
            AuditAction::ResourceRead => {
                self.config.log_fhir_operations && self.config.log_read_operations
            }

            // Search operations
            AuditAction::ResourceSearch => {
                self.config.log_fhir_operations && self.config.log_search_operations
            }

            // Write operations (create, update, delete)
            AuditAction::ResourceCreate
            | AuditAction::ResourceUpdate
            | AuditAction::ResourceDelete => self.config.log_fhir_operations,

            // Admin operations - always log if enabled
            AuditAction::ClientCreate
            | AuditAction::ClientUpdate
            | AuditAction::ClientDelete
            | AuditAction::ConfigChange
            | AuditAction::PolicyEvaluate
            | AuditAction::SystemStartup
            | AuditAction::SystemShutdown => self.config.enabled,
        }
    }

    /// Log an audit event
    pub async fn log(&self, builder: AuditEventBuilder) -> Result<(), AuditError> {
        if !self.config.enabled {
            return Ok(());
        }

        let event = builder.build();
        let id = event["id"].as_str().unwrap_or_default().to_string();

        // Store the AuditEvent resource
        // The event already contains resourceType and id fields
        match self.storage.create(&event).await {
            Ok(_) => {
                tracing::debug!(
                    audit_id = %id,
                    action = %event["subtype"][0]["code"].as_str().unwrap_or("unknown"),
                    "Audit event created"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    audit_id = %id,
                    "Failed to store audit event"
                );
                Err(AuditError::StorageError(e.to_string()))
            }
        }
    }

    /// Log a FHIR resource operation
    pub async fn log_fhir_operation(
        &self,
        action: AuditAction,
        outcome: AuditOutcome,
        resource_type: &str,
        resource_id: Option<&str>,
        query: Option<&str>,
        actor: Option<&ActorType>,
        source: &AuditSource,
        request_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<(), AuditError> {
        // Check if this action should be logged
        if !self.should_log(&action, Some(resource_type)) {
            return Ok(());
        }

        let mut builder = AuditEventBuilder::new(action).outcome(outcome).entity(
            Some(resource_type.to_string()),
            resource_id.map(String::from),
            query.map(String::from),
        );

        if let Some(ip) = source.ip_address {
            builder = builder.ip_address(ip);
        }
        if let Some(ua) = &source.user_agent {
            builder = builder.user_agent(ua.clone());
        }
        if let Some(req_id) = request_id {
            builder = builder.request_id(req_id);
        }
        if let Some(sess_id) = session_id {
            builder = builder.session_id(sess_id);
        }

        match actor {
            Some(ActorType::User {
                id,
                name,
                fhir_user,
            }) => {
                builder = builder.user(id.clone(), name.clone(), fhir_user.clone());
            }
            Some(ActorType::Client { id, name }) => {
                builder = builder.client(id.clone(), name.clone());
            }
            Some(ActorType::System) | None => {
                builder = builder.system();
            }
        }

        self.log(builder).await
    }

    /// Log an authentication event
    pub async fn log_auth_event(
        &self,
        action: AuditAction,
        outcome: AuditOutcome,
        outcome_desc: Option<&str>,
        user_id: Option<&str>,
        username: Option<&str>,
        client_id: Option<&str>,
        source: &AuditSource,
        request_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<(), AuditError> {
        // Check if auth events should be logged
        if !self.should_log(&action, None) {
            return Ok(());
        }

        let mut builder = AuditEventBuilder::new(action).outcome(outcome);

        if let Some(desc) = outcome_desc {
            builder = builder.outcome_desc(desc);
        }

        if let Some(uid) = user_id {
            builder = builder.user(uid, username.map(String::from), None);
        } else if let Some(cid) = client_id {
            builder = builder.client(cid, None);
        } else {
            builder = builder.system();
        }

        if let Some(ip) = source.ip_address {
            builder = builder.ip_address(ip);
        }
        if let Some(ua) = &source.user_agent {
            builder = builder.user_agent(ua.clone());
        }

        if let Some(cid) = client_id {
            builder = builder.client_id(cid);
        }

        if let Some(req_id) = request_id {
            builder = builder.request_id(req_id);
        }

        if let Some(sess_id) = session_id {
            builder = builder.session_id(sess_id);
        }

        self.log(builder).await
    }
}

/// Errors that can occur in the audit service
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("Storage error: {0}")]
    StorageError(String),
}

// =============================================================================
// Helper functions for extracting audit context from HTTP requests
// =============================================================================

/// Extract AuditSource from HTTP headers
pub fn extract_audit_source(headers: &axum::http::HeaderMap) -> AuditSource {
    // Extract client IP from headers (check X-Forwarded-For, X-Real-IP first)
    let ip_address = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()))
        .and_then(|s| s.trim().parse().ok());

    // Extract user agent
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    AuditSource {
        ip_address,
        user_agent,
        site: None,
    }
}

/// Extract request ID from headers (set by request_id middleware)
pub fn extract_request_id(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Extract ActorType from AuthContext
pub fn actor_from_auth_context(auth_context: &octofhir_auth::middleware::AuthContext) -> ActorType {
    if let Some(ref user) = auth_context.user {
        ActorType::User {
            id: user.id.clone(),
            name: Some(user.username.clone()),
            fhir_user: user.fhir_user.clone(),
        }
    } else {
        ActorType::Client {
            id: auth_context.client_id().to_string(),
            name: None,
        }
    }
}

/// Determine audit action from HTTP method and path
pub fn action_from_request(method: &axum::http::Method, path: &str) -> Option<AuditAction> {
    use axum::http::Method;

    // Check for auth paths first
    if path.starts_with("/auth/") {
        return match (method.clone(), path) {
            (m, "/auth/token") if m == Method::POST => Some(AuditAction::UserLogin),
            (m, "/auth/logout") if m == Method::POST => Some(AuditAction::UserLogout),
            _ => None,
        };
    }

    // Skip non-FHIR paths
    if !path.starts_with("/fhir/") && !path.starts_with("/fhir") {
        return None;
    }

    // Extract path after /fhir
    let fhir_path = path.strip_prefix("/fhir").unwrap_or(path);

    // Check for search operations
    if fhir_path.contains("/_search") || (method == Method::GET && fhir_path.contains('?')) {
        return Some(AuditAction::ResourceSearch);
    }

    // Check for history operations
    if fhir_path.contains("/_history") {
        return Some(AuditAction::ResourceRead);
    }

    match *method {
        Method::POST => {
            // POST to /{type} is create, POST to /{type}/_search is search
            if fhir_path.contains("/_search") {
                Some(AuditAction::ResourceSearch)
            } else {
                Some(AuditAction::ResourceCreate)
            }
        }
        Method::GET => Some(AuditAction::ResourceRead),
        Method::PUT => Some(AuditAction::ResourceUpdate),
        Method::PATCH => Some(AuditAction::ResourceUpdate),
        Method::DELETE => Some(AuditAction::ResourceDelete),
        _ => None,
    }
}

/// Extract resource type and ID from FHIR path
pub fn parse_fhir_path(path: &str) -> (Option<String>, Option<String>) {
    let fhir_path = path
        .strip_prefix("/fhir/")
        .or_else(|| path.strip_prefix("/fhir"));

    if let Some(p) = fhir_path {
        let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        match parts.as_slice() {
            [resource_type] => (Some(resource_type.to_string()), None),
            [resource_type, id, ..] => {
                // Skip special paths like _history, _search
                if id.starts_with('_') || id.starts_with('$') {
                    (Some(resource_type.to_string()), None)
                } else {
                    (Some(resource_type.to_string()), Some(id.to_string()))
                }
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    }
}

/// Determine outcome from HTTP status code
pub fn outcome_from_status(status: axum::http::StatusCode) -> AuditOutcome {
    if status.is_success() {
        AuditOutcome::Success
    } else if status.is_client_error() {
        AuditOutcome::MinorFailure
    } else if status.is_server_error() {
        AuditOutcome::SeriousFailure
    } else {
        AuditOutcome::Success
    }
}

/// Log a FHIR operation audit event (called from handlers or middleware)
///
/// This is a convenience function that can be called from FHIR handlers
/// to log audit events for CRUD operations.
pub async fn log_fhir_audit(
    audit_service: &AuditService,
    action: AuditAction,
    outcome: AuditOutcome,
    resource_type: &str,
    resource_id: Option<&str>,
    headers: &axum::http::HeaderMap,
    auth_context: Option<&octofhir_auth::middleware::AuthContext>,
) {
    let source = extract_audit_source(headers);
    let request_id = extract_request_id(headers);
    let actor = auth_context.map(actor_from_auth_context);
    let session_id = auth_context.and_then(|ctx| ctx.token_claims.sid.as_deref());

    if let Err(e) = audit_service
        .log_fhir_operation(
            action,
            outcome,
            resource_type,
            resource_id,
            None,
            actor.as_ref(),
            &source,
            request_id.as_deref(),
            session_id,
        )
        .await
    {
        tracing::warn!(
            error = %e,
            resource_type = %resource_type,
            "Failed to log FHIR audit event"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_builder() {
        let event = AuditEventBuilder::new(AuditAction::ResourceCreate)
            .outcome(AuditOutcome::Success)
            .user(
                Uuid::new_v4(),
                Some("testuser".to_string()),
                Some("Practitioner/123".to_string()),
            )
            .entity(
                Some("Patient".to_string()),
                Some("abc123".to_string()),
                None,
            )
            .ip_address("192.168.1.1".parse().unwrap())
            .request_id("req-001")
            .build();

        assert_eq!(event["resourceType"], "AuditEvent");
        assert_eq!(event["action"], "C");
        assert_eq!(event["outcome"], "0");
        assert_eq!(event["subtype"][0]["code"], "resource.create");
    }

    #[test]
    fn test_action_codes() {
        assert_eq!(AuditAction::ResourceCreate.to_action_code(), "C");
        assert_eq!(AuditAction::ResourceRead.to_action_code(), "R");
        assert_eq!(AuditAction::ResourceUpdate.to_action_code(), "U");
        assert_eq!(AuditAction::ResourceDelete.to_action_code(), "D");
        assert_eq!(AuditAction::UserLogin.to_action_code(), "E");
    }
}
