//! Rhai scripting engine for policy evaluation.
//!
//! This module provides a sandboxed Rhai runtime for executing policy scripts
//! with access to the policy evaluation context.
//!
//! # Design
//!
//! The Rhai engine is **created once** at startup and reused for all evaluations.
//! Scripts are compiled to AST once and cached. Only the evaluation scope is
//! created per-request, which is a lightweight operation.
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ RhaiRuntime (created once at startup)   │
//! │   ├── engine: Engine (configured once)  │
//! │   └── script_cache: HashMap<hash, AST>  │
//! └─────────────────────────────────────────┘
//!              │
//!              ▼ evaluate(script, context)
//! ┌─────────────────────────────────────────┐
//! │ Per-request (lightweight):              │
//! │   1. Get or compile AST (cached)        │
//! │   2. Create fresh Scope with context    │
//! │   3. Evaluate AST with scope            │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Helper Functions
//!
//! The following helper functions are available in policy scripts:
//!
//! - `has_role(user, role)` - Check if user has a specific role
//! - `has_any_role(user, roles)` - Check if user has any of the specified roles
//! - `is_patient_user(user)` - Check if user's FHIR type is Patient
//! - `is_practitioner_user(user)` - Check if user's FHIR type is Practitioner
//! - `get_resource_subject(resource)` - Get subject reference from resource
//! - `in_patient_compartment(context)` - Check if request is in patient compartment
//! - `allow()` - Return an allow decision
//! - `deny(reason)` - Return a deny decision with reason
//! - `abstain()` - Return an abstain decision

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::RwLock;

use rhai::{AST, Dynamic, Engine, Map, Scope};

use crate::config::RhaiConfig;
use crate::policy::context::PolicyContext;
use crate::policy::engine::{AccessDecision, DenyReason};

// =============================================================================
// Rhai Runtime
// =============================================================================

/// Rhai scripting runtime for policy evaluation.
///
/// This struct holds the Rhai engine and script cache. The engine is configured
/// once at creation with sandbox limits and helper functions. Scripts are compiled
/// to AST on first use and cached for subsequent evaluations.
pub struct RhaiRuntime {
    /// Rhai engine with sandbox configuration.
    engine: Engine,

    /// Cache of compiled ASTs by script hash.
    script_cache: RwLock<HashMap<u64, AST>>,

    /// Runtime configuration.
    #[allow(dead_code)]
    config: RhaiConfig,
}

impl RhaiRuntime {
    /// Create a new Rhai runtime with the given configuration.
    ///
    /// This configures the engine with sandbox limits and registers helper functions.
    /// The engine is created once and reused for all script evaluations.
    #[must_use]
    pub fn new(config: RhaiConfig) -> Self {
        let mut engine = Engine::new();

        // Configure sandbox limits
        engine.set_max_operations(config.max_operations);
        engine.set_max_call_levels(config.max_call_levels);
        engine.set_max_expr_depths(64, 64);
        engine.set_max_string_size(10_000);
        engine.set_max_array_size(1_000);
        engine.set_max_map_size(1_000);

        // Disable dangerous features
        engine.disable_symbol("eval");

        // Register helper functions
        Self::register_helpers(&mut engine);

        Self {
            engine,
            script_cache: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Evaluate a policy script with the given context.
    ///
    /// Returns an `AccessDecision` based on the script result:
    /// - `true` → Allow
    /// - `false` → Deny (generic "script returned false")
    /// - Decision map with `decision` field → parsed decision
    /// - Error → Deny with script error
    pub fn evaluate(&self, script: &str, context: &PolicyContext) -> AccessDecision {
        // Get or compile the AST (cached)
        let ast = match self.get_or_compile_ast(script) {
            Ok(ast) => ast,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to compile Rhai script");
                return AccessDecision::Deny(DenyReason::script_error(
                    "unknown",
                    format!("Script compilation failed: {}", e),
                ));
            }
        };

        // Create a fresh scope with context variables
        let mut scope = Scope::new();
        self.populate_scope(&mut scope, context);

        // Evaluate the script
        match self.engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
            Ok(result) => self.parse_result(result),
            Err(e) => {
                tracing::warn!(error = %e, "Rhai script evaluation failed");
                AccessDecision::Deny(DenyReason::script_error(
                    "unknown",
                    format!("Script execution failed: {}", e),
                ))
            }
        }
    }

    /// Get a compiled AST from cache or compile and cache it.
    fn get_or_compile_ast(&self, script: &str) -> Result<AST, Box<rhai::EvalAltResult>> {
        let hash = Self::hash_script(script);

        // Check cache first (read lock)
        {
            let cache = self.script_cache.read().unwrap();
            if let Some(ast) = cache.get(&hash) {
                return Ok(ast.clone());
            }
        }

        // Compile and cache (write lock)
        let ast = self.engine.compile(script)?;

        {
            let mut cache = self.script_cache.write().unwrap();
            cache.insert(hash, ast.clone());
        }

        Ok(ast)
    }

    /// Compute a hash of a script for cache lookup.
    fn hash_script(script: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        script.hash(&mut hasher);
        hasher.finish()
    }

    /// Register helper functions in the Rhai engine.
    fn register_helpers(engine: &mut Engine) {
        // Role checking
        engine.register_fn("has_role", |user: Map, role: &str| -> bool {
            user.get("roles")
                .and_then(|r| r.clone().try_cast::<rhai::Array>())
                .map(|roles| {
                    roles
                        .iter()
                        .any(|r| r.clone().into_string().ok() == Some(role.to_string()))
                })
                .unwrap_or(false)
        });

        engine.register_fn("has_any_role", |user: Map, roles: rhai::Array| -> bool {
            let user_roles = user
                .get("roles")
                .and_then(|r| r.clone().try_cast::<rhai::Array>())
                .unwrap_or_default();

            roles.iter().any(|check_role| {
                let check_str = check_role.clone().into_string().ok();
                user_roles
                    .iter()
                    .any(|ur| ur.clone().into_string().ok() == check_str)
            })
        });

        // User type checking
        engine.register_fn("is_patient_user", |user: Map| -> bool {
            user.get("fhirUserType")
                .and_then(|t| t.clone().into_string().ok())
                .map(|t| t == "Patient")
                .unwrap_or(false)
        });

        engine.register_fn("is_practitioner_user", |user: Map| -> bool {
            user.get("fhirUserType")
                .and_then(|t| t.clone().into_string().ok())
                .map(|t| t == "Practitioner")
                .unwrap_or(false)
        });

        // Resource helpers
        engine.register_fn("get_resource_subject", |resource: Map| -> Dynamic {
            resource.get("subject").cloned().unwrap_or(Dynamic::UNIT)
        });

        // Compartment checking
        engine.register_fn("in_patient_compartment", |context: Map| -> bool {
            // Check if patient context matches resource subject or compartment
            let patient_context = context
                .get("environment")
                .and_then(|e| e.clone().try_cast::<Map>())
                .and_then(|e| e.get("patientContext").cloned())
                .and_then(|p| p.into_string().ok());

            let Some(patient) = patient_context else {
                return false;
            };

            // Check compartment from request
            let request = context
                .get("request")
                .and_then(|r| r.clone().try_cast::<Map>());

            if let Some(req) = &request
                && let Some(comp_type) = req
                    .get("compartmentType")
                    .and_then(|t| t.clone().into_string().ok())
                && comp_type == "Patient"
                && let Some(comp_id) = req
                    .get("compartmentId")
                    .and_then(|i| i.clone().into_string().ok())
            {
                let full_ref = format!("Patient/{}", comp_id);
                if full_ref == patient || comp_id == patient {
                    return true;
                }
            }

            // Check resource subject
            let resource = context
                .get("resource")
                .and_then(|r| r.clone().try_cast::<Map>());

            if let Some(res) = resource
                && let Some(subject) = res
                    .get("subject")
                    .and_then(|s| s.clone().into_string().ok())
                && (subject == patient || subject.ends_with(&format!("/{}", patient)))
            {
                return true;
            }

            false
        });

        // Decision functions
        engine.register_fn("allow", || -> Map {
            let mut map = Map::new();
            map.insert("decision".into(), "allow".into());
            map
        });

        engine.register_fn("deny", |reason: &str| -> Map {
            let mut map = Map::new();
            map.insert("decision".into(), "deny".into());
            map.insert("reason".into(), reason.into());
            map
        });

        engine.register_fn("abstain", || -> Map {
            let mut map = Map::new();
            map.insert("decision".into(), "abstain".into());
            map
        });
    }

    /// Populate a Rhai scope with context variables.
    fn populate_scope(&self, scope: &mut Scope, context: &PolicyContext) {
        // Convert context to Dynamic maps
        scope.push("user", self.user_to_dynamic(&context.user));
        scope.push("client", self.client_to_dynamic(&context.client));
        scope.push("request", self.request_to_dynamic(&context.request));
        scope.push("scopes", self.scopes_to_dynamic(&context.scopes));
        scope.push(
            "environment",
            self.environment_to_dynamic(&context.environment),
        );

        if let Some(ref resource) = context.resource {
            scope.push("resource", self.resource_to_dynamic(resource));
        } else {
            scope.push("resource", Dynamic::UNIT);
        }

        // Also push the full context for helper functions
        scope.push("context", self.context_to_dynamic(context));
    }

    /// Parse a script result into an AccessDecision.
    fn parse_result(&self, result: Dynamic) -> AccessDecision {
        // Boolean result
        if let Ok(b) = result.as_bool() {
            return if b {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny(DenyReason {
                    code: "script-denied".to_string(),
                    message: "Policy script returned false".to_string(),
                    details: None,
                    policy_id: None,
                })
            };
        }

        // Map result with decision field
        if let Some(map) = result.try_cast::<Map>()
            && let Some(decision) = map
                .get("decision")
                .and_then(|d| d.clone().into_string().ok())
        {
            return match decision.as_str() {
                "allow" => AccessDecision::Allow,
                "deny" => {
                    let reason = map
                        .get("reason")
                        .and_then(|r| r.clone().into_string().ok())
                        .unwrap_or_else(|| "Access denied by policy script".to_string());
                    AccessDecision::Deny(DenyReason {
                        code: "script-denied".to_string(),
                        message: reason,
                        details: None,
                        policy_id: None,
                    })
                }
                "abstain" => AccessDecision::Abstain,
                _ => AccessDecision::Abstain,
            };
        }

        // Unknown result type - abstain
        tracing::warn!("Rhai script returned unexpected type, abstaining");
        AccessDecision::Abstain
    }

    /// Convert user identity to Rhai Dynamic.
    fn user_to_dynamic(&self, user: &Option<crate::policy::context::UserIdentity>) -> Dynamic {
        let Some(user) = user else {
            return Dynamic::UNIT;
        };

        let mut map = Map::new();
        map.insert("id".into(), user.id.clone().into());

        if let Some(ref fhir_user) = user.fhir_user {
            map.insert("fhirUser".into(), fhir_user.clone().into());
        }
        if let Some(ref fhir_user_type) = user.fhir_user_type {
            map.insert("fhirUserType".into(), fhir_user_type.clone().into());
        }
        if let Some(ref fhir_user_id) = user.fhir_user_id {
            map.insert("fhirUserId".into(), fhir_user_id.clone().into());
        }

        let roles: rhai::Array = user
            .roles
            .iter()
            .map(|r| Dynamic::from(r.clone()))
            .collect();
        map.insert("roles".into(), roles.into());

        map.into()
    }

    /// Convert client identity to Rhai Dynamic.
    fn client_to_dynamic(&self, client: &crate::policy::context::ClientIdentity) -> Dynamic {
        let mut map = Map::new();
        map.insert("id".into(), client.id.clone().into());
        map.insert("name".into(), client.name.clone().into());
        map.insert("trusted".into(), client.trusted.into());
        map.insert(
            "clientType".into(),
            format!("{:?}", client.client_type).into(),
        );
        map.into()
    }

    /// Convert request context to Rhai Dynamic.
    fn request_to_dynamic(&self, request: &crate::policy::context::RequestContext) -> Dynamic {
        let mut map = Map::new();
        map.insert(
            "operation".into(),
            format!("{:?}", request.operation).into(),
        );
        map.insert("resourceType".into(), request.resource_type.clone().into());
        map.insert("path".into(), request.path.clone().into());
        map.insert("method".into(), request.method.clone().into());

        if let Some(ref id) = request.resource_id {
            map.insert("resourceId".into(), id.clone().into());
        }
        if let Some(ref ct) = request.compartment_type {
            map.insert("compartmentType".into(), ct.clone().into());
        }
        if let Some(ref cid) = request.compartment_id {
            map.insert("compartmentId".into(), cid.clone().into());
        }

        // Convert query params
        let mut params = Map::new();
        for (k, v) in &request.query_params {
            params.insert(k.clone().into(), v.clone().into());
        }
        map.insert("queryParams".into(), params.into());

        map.into()
    }

    /// Convert scope summary to Rhai Dynamic.
    fn scopes_to_dynamic(&self, scopes: &crate::policy::context::ScopeSummary) -> Dynamic {
        let mut map = Map::new();
        map.insert("raw".into(), scopes.raw.clone().into());
        map.insert("hasWildcard".into(), scopes.has_wildcard.into());
        map.insert("launch".into(), scopes.launch.into());
        map.insert("openid".into(), scopes.openid.into());
        map.insert("fhirUser".into(), scopes.fhir_user.into());
        map.insert("offlineAccess".into(), scopes.offline_access.into());

        let patient: rhai::Array = scopes
            .patient_scopes
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect();
        map.insert("patientScopes".into(), patient.into());

        let user: rhai::Array = scopes
            .user_scopes
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect();
        map.insert("userScopes".into(), user.into());

        let system: rhai::Array = scopes
            .system_scopes
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect();
        map.insert("systemScopes".into(), system.into());

        map.into()
    }

    /// Convert environment context to Rhai Dynamic.
    fn environment_to_dynamic(&self, env: &crate::policy::context::EnvironmentContext) -> Dynamic {
        let mut map = Map::new();
        map.insert("requestId".into(), env.request_id.clone().into());
        map.insert("requestTime".into(), env.request_time.to_string().into());

        if let Some(ref ip) = env.source_ip {
            map.insert("sourceIp".into(), ip.to_string().into());
        }
        if let Some(ref patient) = env.patient_context {
            map.insert("patientContext".into(), patient.clone().into());
        }
        if let Some(ref encounter) = env.encounter_context {
            map.insert("encounterContext".into(), encounter.clone().into());
        }

        map.into()
    }

    /// Convert resource context to Rhai Dynamic.
    fn resource_to_dynamic(&self, resource: &crate::policy::context::ResourceContext) -> Dynamic {
        let mut map = Map::new();
        map.insert("id".into(), resource.id.clone().into());
        map.insert("resourceType".into(), resource.resource_type.clone().into());

        if let Some(ref vid) = resource.version_id {
            map.insert("versionId".into(), vid.clone().into());
        }
        if let Some(ref updated) = resource.last_updated {
            map.insert("lastUpdated".into(), updated.clone().into());
        }
        if let Some(ref subject) = resource.subject {
            map.insert("subject".into(), subject.clone().into());
        }
        if let Some(ref author) = resource.author {
            map.insert("author".into(), author.clone().into());
        }

        // Also include the raw resource JSON as a nested map
        map.insert("data".into(), self.json_to_dynamic(&resource.resource));

        map.into()
    }

    /// Convert the full PolicyContext to Rhai Dynamic (for helper functions).
    fn context_to_dynamic(&self, context: &PolicyContext) -> Dynamic {
        let mut map = Map::new();
        map.insert("user".into(), self.user_to_dynamic(&context.user));
        map.insert("client".into(), self.client_to_dynamic(&context.client));
        map.insert("request".into(), self.request_to_dynamic(&context.request));
        map.insert("scopes".into(), self.scopes_to_dynamic(&context.scopes));
        map.insert(
            "environment".into(),
            self.environment_to_dynamic(&context.environment),
        );

        if let Some(ref resource) = context.resource {
            map.insert("resource".into(), self.resource_to_dynamic(resource));
        }

        map.into()
    }

    /// Convert a serde_json::Value to Rhai Dynamic.
    #[allow(clippy::only_used_in_recursion)]
    fn json_to_dynamic(&self, value: &serde_json::Value) -> Dynamic {
        match value {
            serde_json::Value::Null => Dynamic::UNIT,
            serde_json::Value::Bool(b) => Dynamic::from(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Dynamic::from(i)
                } else if let Some(f) = n.as_f64() {
                    Dynamic::from(f)
                } else {
                    Dynamic::UNIT
                }
            }
            serde_json::Value::String(s) => Dynamic::from(s.clone()),
            serde_json::Value::Array(arr) => {
                let rhai_arr: rhai::Array = arr.iter().map(|v| self.json_to_dynamic(v)).collect();
                Dynamic::from(rhai_arr)
            }
            serde_json::Value::Object(obj) => {
                let mut map = Map::new();
                for (k, v) in obj {
                    map.insert(k.clone().into(), self.json_to_dynamic(v));
                }
                Dynamic::from(map)
            }
        }
    }

    /// Get cache statistics for monitoring.
    #[must_use]
    pub fn cache_stats(&self) -> RhaiCacheStats {
        let cache = self.script_cache.read().unwrap();
        RhaiCacheStats {
            cached_scripts: cache.len(),
        }
    }

    /// Clear the script cache.
    pub fn clear_cache(&self) {
        let mut cache = self.script_cache.write().unwrap();
        cache.clear();
    }
}

/// Statistics about the Rhai script cache.
#[derive(Debug, Clone)]
pub struct RhaiCacheStats {
    /// Number of cached scripts.
    pub cached_scripts: usize,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::context::{
        ClientIdentity, ClientType, EnvironmentContext, RequestContext, ScopeSummary, UserIdentity,
    };
    use crate::smart::scopes::FhirOperation;
    use time::OffsetDateTime;

    fn create_test_context() -> PolicyContext {
        PolicyContext {
            user: Some(UserIdentity {
                id: "user-123".to_string(),
                fhir_user: Some("Practitioner/456".to_string()),
                fhir_user_type: Some("Practitioner".to_string()),
                fhir_user_id: Some("456".to_string()),
                roles: vec!["doctor".to_string(), "admin".to_string()],
                attributes: std::collections::HashMap::new(),
            }),
            client: ClientIdentity {
                id: "client-123".to_string(),
                name: "Test Client".to_string(),
                trusted: false,
                client_type: ClientType::Public,
            },
            scopes: ScopeSummary {
                raw: "user/Patient.r".to_string(),
                patient_scopes: vec![],
                user_scopes: vec!["user/Patient.r".to_string()],
                system_scopes: vec![],
                has_wildcard: false,
                launch: false,
                openid: false,
                fhir_user: false,
                offline_access: false,
            },
            request: RequestContext {
                operation: FhirOperation::Read,
                resource_type: "Patient".to_string(),
                resource_id: Some("pat-123".to_string()),
                compartment_type: None,
                compartment_id: None,
                body: None,
                query_params: std::collections::HashMap::new(),
                path: "/Patient/pat-123".to_string(),
                method: "GET".to_string(),
            },
            resource: None,
            environment: EnvironmentContext {
                request_time: OffsetDateTime::now_utc(),
                source_ip: None,
                request_id: "req-123".to_string(),
                patient_context: Some("Patient/pat-123".to_string()),
                encounter_context: None,
            },
        }
    }

    #[test]
    fn test_simple_allow() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        let result = runtime.evaluate("true", &context);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_simple_deny() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        let result = runtime.evaluate("false", &context);
        assert!(result.is_denied());
    }

    #[test]
    fn test_role_check() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // User has "doctor" role
        let result = runtime.evaluate(r#"has_role(user, "doctor")"#, &context);
        assert!(result.is_allowed());

        // User doesn't have "nurse" role
        let result = runtime.evaluate(r#"has_role(user, "nurse")"#, &context);
        assert!(result.is_denied());
    }

    #[test]
    fn test_has_any_role() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // User has one of the roles
        let result = runtime.evaluate(r#"has_any_role(user, ["nurse", "doctor"])"#, &context);
        assert!(result.is_allowed());

        // User doesn't have any of the roles
        let result = runtime.evaluate(r#"has_any_role(user, ["nurse", "receptionist"])"#, &context);
        assert!(result.is_denied());
    }

    #[test]
    fn test_user_type_check() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // User is a Practitioner
        let result = runtime.evaluate("is_practitioner_user(user)", &context);
        assert!(result.is_allowed());

        // User is not a Patient
        let result = runtime.evaluate("is_patient_user(user)", &context);
        assert!(result.is_denied());
    }

    #[test]
    fn test_decision_functions() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // allow() function
        let result = runtime.evaluate("allow()", &context);
        assert!(result.is_allowed());

        // deny() function
        let result = runtime.evaluate(r#"deny("Custom denial reason")"#, &context);
        assert!(result.is_denied());
        if let AccessDecision::Deny(reason) = result {
            assert_eq!(reason.message, "Custom denial reason");
        }

        // abstain() function
        let result = runtime.evaluate("abstain()", &context);
        assert!(result.is_abstain());
    }

    #[test]
    fn test_conditional_logic() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // Complex conditional
        let script = r#"
            if has_role(user, "admin") {
                allow()
            } else if request.operation == "Read" {
                allow()
            } else {
                deny("Access denied")
            }
        "#;

        let result = runtime.evaluate(script, &context);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_access_context_variables() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // Access user fields
        let result = runtime.evaluate(r#"user.id == "user-123""#, &context);
        assert!(result.is_allowed());

        // Access client fields
        let result = runtime.evaluate(r#"client.id == "client-123""#, &context);
        assert!(result.is_allowed());

        // Access request fields
        let result = runtime.evaluate(r#"request.resourceType == "Patient""#, &context);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_script_caching() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // First evaluation compiles the script
        let _ = runtime.evaluate("true", &context);
        assert_eq!(runtime.cache_stats().cached_scripts, 1);

        // Second evaluation uses cached AST
        let _ = runtime.evaluate("true", &context);
        assert_eq!(runtime.cache_stats().cached_scripts, 1);

        // Different script adds to cache
        let _ = runtime.evaluate("false", &context);
        assert_eq!(runtime.cache_stats().cached_scripts, 2);
    }

    #[test]
    fn test_script_error_handling() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        // Syntax error
        let result = runtime.evaluate("if { }", &context);
        assert!(result.is_denied());
        if let AccessDecision::Deny(reason) = result {
            assert_eq!(reason.code, "script-error");
        }

        // Runtime error (undefined variable)
        let result = runtime.evaluate("undefined_var", &context);
        assert!(result.is_denied());
    }

    #[test]
    fn test_script_timeout() {
        // Configure with very low operation limit
        let config = RhaiConfig {
            max_operations: 100,
            max_call_levels: 32,
        };
        let runtime = RhaiRuntime::new(config);
        let context = create_test_context();

        // Infinite loop should be stopped by max_operations
        let result = runtime.evaluate("loop { }", &context);
        assert!(result.is_denied());
        if let AccessDecision::Deny(reason) = result {
            assert_eq!(reason.code, "script-error");
        }
    }

    #[test]
    fn test_null_user() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let mut context = create_test_context();
        context.user = None;

        // Script should handle null user gracefully
        let result = runtime.evaluate("user == ()", &context);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_clear_cache() {
        let runtime = RhaiRuntime::new(RhaiConfig::default());
        let context = create_test_context();

        let _ = runtime.evaluate("true", &context);
        let _ = runtime.evaluate("false", &context);
        assert_eq!(runtime.cache_stats().cached_scripts, 2);

        runtime.clear_cache();
        assert_eq!(runtime.cache_stats().cached_scripts, 0);
    }
}
