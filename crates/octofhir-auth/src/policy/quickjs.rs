//! QuickJS scripting engine for policy evaluation.
//!
//! This module provides a sandboxed QuickJS JavaScript runtime for executing
//! policy scripts with access to the policy evaluation context.
//!
//! # Design
//!
//! The QuickJS runtime uses a **pool of runtimes** for parallel execution.
//! Each runtime in the pool has its own Context and is protected by a mutex.
//! Round-robin selection distributes load across the pool.
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │ QuickJsRuntime (pool, created once at startup)  │
//! │   ├── instances: Vec<Mutex<QuickJsInstance>>    │
//! │   ├── config: QuickJsConfig                     │
//! │   └── counter: AtomicUsize (round-robin)        │
//! └─────────────────────────────────────────────────┘
//!              │
//!              ▼ evaluate(script, context)
//! ┌─────────────────────────────────────────────────┐
//! │ Per-request:                                    │
//! │   1. Get runtime instance (round-robin)         │
//! │   2. Lock instance (Mutex)                      │
//! │   3. Set interrupt handler for timeout          │
//! │   4. Inject context variables                   │
//! │   5. Execute user script with helpers           │
//! │   6. Clear interrupt handler                    │
//! │   7. Parse result → AccessDecision              │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Helper Functions
//!
//! The following helper functions are available in policy scripts:
//!
//! - `allow()` - Return an allow decision
//! - `deny(reason)` - Return a deny decision with reason
//! - `abstain()` - Return an abstain decision
//! - `hasRole(role)` - Check if user has a specific role
//! - `hasAnyRole(...roles)` - Check if user has any of the roles
//! - `isPatientUser()` - Check if user's FHIR type is Patient
//! - `isPractitionerUser()` - Check if user's FHIR type is Practitioner
//! - `inPatientCompartment()` - Check if request is in patient compartment
//! - `console.log/warn/error` - Logging (mapped to tracing)

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rquickjs::{Context, Ctx, FromJs, Function, Object, Runtime, Value};

use crate::config::QuickJsConfig;
use crate::policy::context::PolicyContext;
use crate::policy::engine::{AccessDecision, DenyReason};

// =============================================================================
// QuickJS Runtime Pool
// =============================================================================

/// QuickJS runtime pool for policy evaluation.
///
/// This struct manages a pool of QuickJS runtime instances for parallel
/// policy script evaluation. Each instance is protected by a mutex and
/// configured with memory/stack limits.
pub struct QuickJsRuntime {
    /// Pool of runtime instances.
    instances: Vec<Mutex<QuickJsInstance>>,

    /// Runtime configuration.
    config: QuickJsConfig,

    /// Round-robin counter for instance selection.
    counter: AtomicUsize,
}

/// Individual QuickJS runtime instance.
struct QuickJsInstance {
    /// QuickJS runtime with limits configured.
    runtime: Runtime,

    /// Persistent context for script evaluation.
    context: Context,
}

impl QuickJsRuntime {
    /// Create a new QuickJS runtime pool with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if any runtime instance fails to initialize.
    pub fn new(config: QuickJsConfig) -> Result<Self, QuickJsError> {
        let pool_size = config.pool_size.max(1);
        let mut instances = Vec::with_capacity(pool_size);

        for _ in 0..pool_size {
            let runtime = Runtime::new().map_err(|e| QuickJsError::InitError(e.to_string()))?;

            // Set memory limit (bytes)
            runtime.set_memory_limit(config.memory_limit_mb * 1024 * 1024);

            // Set stack limit (bytes)
            runtime.set_max_stack_size(config.max_stack_size_kb * 1024);

            // Create context
            let context =
                Context::full(&runtime).map_err(|e| QuickJsError::InitError(e.to_string()))?;

            instances.push(Mutex::new(QuickJsInstance { runtime, context }));
        }

        Ok(Self {
            instances,
            config,
            counter: AtomicUsize::new(0),
        })
    }

    /// Evaluate a policy script with the given context.
    ///
    /// Returns an `AccessDecision` based on the script result:
    /// - `true` → Allow
    /// - `false` → Deny (generic reason)
    /// - Decision object with `decision` field → parsed decision
    /// - Error → Deny with script error
    pub fn evaluate(&self, script: &str, context: &PolicyContext) -> AccessDecision {
        let instance_idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.instances.len();

        let instance = match self.instances[instance_idx].lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // Recover from poisoned mutex
                tracing::warn!("QuickJS instance mutex was poisoned, recovering");
                poisoned.into_inner()
            }
        };

        self.evaluate_with_instance(&instance, script, context)
    }

    /// Evaluate script using a specific instance.
    fn evaluate_with_instance(
        &self,
        instance: &QuickJsInstance,
        script: &str,
        context: &PolicyContext,
    ) -> AccessDecision {
        // Set up interrupt handler for timeout
        let start = Instant::now();
        let timeout_ms = self.config.timeout_ms;

        instance
            .runtime
            .set_interrupt_handler(Some(Box::new(move || {
                start.elapsed().as_millis() > timeout_ms as u128
            })));

        // Execute in context
        let result = instance
            .context
            .with(|ctx| self.execute_script(ctx, script, context));

        // Clear interrupt handler
        instance.runtime.set_interrupt_handler(None);

        result
    }

    /// Execute script within context.
    fn execute_script(
        &self,
        ctx: Ctx<'_>,
        script: &str,
        policy_context: &PolicyContext,
    ) -> AccessDecision {
        // Inject context as JSON
        if let Err(e) = self.inject_context(&ctx, policy_context) {
            tracing::warn!(error = %e, "Failed to inject context into QuickJS");
            return AccessDecision::Deny(DenyReason {
                code: "script-error".to_string(),
                message: format!("Failed to inject context: {}", e),
                details: None,
                policy_id: None,
            });
        }

        // Setup console logging
        if let Err(e) = self.setup_console(&ctx) {
            tracing::warn!(error = %e, "Failed to setup console in QuickJS");
        }

        // Wrap user script with helper functions
        let wrapped_script = format!(
            r#"
(function() {{
    // Decision helper functions
    const allow = () => ({{ decision: "allow" }});
    const deny = (reason) => ({{ decision: "deny", reason: reason || "Access denied" }});
    const abstain = () => ({{ decision: "abstain" }});

    // Role checking helpers
    const hasRole = (role) => user && user.roles && user.roles.includes(role);
    const hasAnyRole = (...roles) => roles.some(r => hasRole(r));

    // User type helpers
    const isPatientUser = () => user && user.fhirUserType === "Patient";
    const isPractitionerUser = () => user && user.fhirUserType === "Practitioner";

    // Context helpers
    const getPatientContext = () => environment.patientContext;
    const getEncounterContext = () => environment.encounterContext;

    // Compartment check helper
    const inPatientCompartment = () => {{
        const patientId = environment.patientContext;
        if (!patientId) return false;
        if (!resource || !resource.subject) return false;
        const ref = resource.subject;
        return ref === `Patient/${{patientId}}` || ref.endsWith(`/${{patientId}}`);
    }};

    // User's policy script
    {script}
}})()
"#
        );

        // Evaluate the wrapped script
        match ctx.eval::<Value, _>(wrapped_script.as_bytes()) {
            Ok(result) => self.parse_result(&ctx, result),
            Err(e) => self.handle_error(e),
        }
    }

    /// Inject policy context into JavaScript global scope.
    fn inject_context(&self, ctx: &Ctx<'_>, policy_context: &PolicyContext) -> Result<(), String> {
        let globals = ctx.globals();

        // Serialize context to JSON
        let context_json = serde_json::to_string(policy_context).map_err(|e| e.to_string())?;

        // Create setup script that parses the JSON and extracts variables
        // Use var instead of const to allow re-assignment in subsequent evaluations
        let setup_script = format!(
            r#"
var __ctx = {};
var user = __ctx.user || null;
var client = __ctx.client;
var scopes = __ctx.scopes;
var request = __ctx.request;
var resource = __ctx.resource;
var environment = __ctx.environment;
"#,
            context_json
        );

        ctx.eval::<(), _>(setup_script.as_bytes())
            .map_err(|e| e.to_string())?;

        // Clear the temporary context object
        globals.remove("__ctx").ok();

        Ok(())
    }

    /// Setup console.log/warn/error for debugging.
    fn setup_console(&self, ctx: &Ctx<'_>) -> Result<(), rquickjs::Error> {
        let globals = ctx.globals();

        let console = Object::new(ctx.clone())?;

        // console.log
        console.set(
            "log",
            Function::new(ctx.clone(), |msg: String| {
                tracing::debug!(target: "quickjs", message = %msg, "console.log");
            })?,
        )?;

        // console.warn
        console.set(
            "warn",
            Function::new(ctx.clone(), |msg: String| {
                tracing::warn!(target: "quickjs", message = %msg, "console.warn");
            })?,
        )?;

        // console.error
        console.set(
            "error",
            Function::new(ctx.clone(), |msg: String| {
                tracing::error!(target: "quickjs", message = %msg, "console.error");
            })?,
        )?;

        globals.set("console", console)?;
        Ok(())
    }

    /// Parse JavaScript result into AccessDecision.
    fn parse_result<'js>(&self, ctx: &Ctx<'js>, value: Value<'js>) -> AccessDecision {
        // Try as boolean first
        if let Ok(b) = bool::from_js(ctx, value.clone()) {
            return if b {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny(DenyReason {
                    code: "script-denied".to_string(),
                    message: "Access denied by policy".to_string(),
                    details: None,
                    policy_id: None,
                })
            };
        }

        // Try as object with decision field
        if let Some(obj) = value.into_object()
            && let Ok(decision) = obj.get::<_, String>("decision")
        {
            return match decision.as_str() {
                "allow" => AccessDecision::Allow,
                "deny" => {
                    let reason = obj
                        .get::<_, String>("reason")
                        .unwrap_or_else(|_| "Access denied".to_string());
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
        tracing::warn!("QuickJS script returned unexpected type, abstaining");
        AccessDecision::Abstain
    }

    /// Handle JavaScript execution errors.
    fn handle_error(&self, error: rquickjs::Error) -> AccessDecision {
        let error_str = error.to_string();
        let error_lower = error_str.to_lowercase();

        // Check for various interrupt/timeout patterns
        if error_lower.contains("interrupt")
            || error_lower.contains("timeout")
            || error_lower.contains("aborted")
        {
            tracing::warn!("QuickJS script timeout");
            AccessDecision::Deny(DenyReason {
                code: "script-timeout".to_string(),
                message: "Policy script execution timeout".to_string(),
                details: None,
                policy_id: None,
            })
        } else if error_lower.contains("out of memory")
            || error_lower.contains("memory")
            || error_lower.contains("stack")
        {
            tracing::warn!("QuickJS script memory limit exceeded");
            AccessDecision::Deny(DenyReason {
                code: "script-memory-limit".to_string(),
                message: "Policy script exceeded memory limit".to_string(),
                details: None,
                policy_id: None,
            })
        } else {
            tracing::error!(error = %error_str, "QuickJS script error");
            AccessDecision::Deny(DenyReason {
                code: "script-error".to_string(),
                message: format!("Policy script error: {}", error_str),
                details: None,
                policy_id: None,
            })
        }
    }

    /// Get runtime pool statistics.
    #[must_use]
    pub fn stats(&self) -> QuickJsCacheStats {
        QuickJsCacheStats {
            pool_size: self.instances.len(),
            evaluations: self.counter.load(Ordering::Relaxed),
        }
    }
}

// =============================================================================
// Statistics
// =============================================================================

/// Statistics about the QuickJS runtime pool.
#[derive(Debug, Clone)]
pub struct QuickJsCacheStats {
    /// Number of runtime instances in the pool.
    pub pool_size: usize,

    /// Total number of evaluations performed.
    pub evaluations: usize,
}

// =============================================================================
// Errors
// =============================================================================

/// QuickJS runtime errors.
#[derive(Debug, thiserror::Error)]
pub enum QuickJsError {
    /// Initialization failed.
    #[error("QuickJS initialization failed: {0}")]
    InitError(String),

    /// Runtime error during script execution.
    #[error("QuickJS runtime error: {0}")]
    RuntimeError(String),
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::context::{
        ClientIdentity, ClientType, EnvironmentContext, RequestContext, ResourceContext,
        ScopeSummary, UserIdentity,
    };
    use crate::smart::scopes::FhirOperation;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn create_test_context() -> PolicyContext {
        PolicyContext {
            user: Some(UserIdentity {
                id: "user-123".to_string(),
                fhir_user: Some("Practitioner/456".to_string()),
                fhir_user_type: Some("Practitioner".to_string()),
                fhir_user_id: Some("456".to_string()),
                roles: vec!["doctor".to_string(), "admin".to_string()],
                attributes: HashMap::new(),
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
                query_params: HashMap::new(),
                path: "/Patient/pat-123".to_string(),
                method: "GET".to_string(),
                operation_id: None,
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

    fn create_context_with_role(role: &str) -> PolicyContext {
        let mut ctx = create_test_context();
        if let Some(ref mut user) = ctx.user {
            user.roles = vec![role.to_string()];
        }
        ctx
    }

    #[test]
    fn test_quickjs_simple_allow() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        let decision = runtime.evaluate("return allow();", &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_simple_deny() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        let decision = runtime.evaluate(r#"return deny("Custom reason");"#, &context);
        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.message, "Custom reason");
        }
    }

    #[test]
    fn test_quickjs_abstain() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        let decision = runtime.evaluate("return abstain();", &context);
        assert!(decision.is_abstain());
    }

    #[test]
    fn test_quickjs_boolean_return() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        // true -> allow
        let decision = runtime.evaluate("return true;", &context);
        assert!(decision.is_allowed());

        // false -> deny
        let decision = runtime.evaluate("return false;", &context);
        assert!(decision.is_denied());
    }

    #[test]
    fn test_quickjs_role_check() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_context_with_role("admin");

        let script = r#"
            if (hasRole("admin")) {
                return allow();
            }
            return deny("Not admin");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_role_check_failure() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_context_with_role("user");

        let script = r#"
            if (hasRole("admin")) {
                return allow();
            }
            return deny("Not admin");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_denied());
    }

    #[test]
    fn test_quickjs_has_any_role() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        // User has "doctor" role
        let script = r#"
            if (hasAnyRole("nurse", "doctor")) {
                return allow();
            }
            return deny("No matching role");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_context_access() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        let script = r#"
            if (request.resourceType === "Patient" && request.method === "GET") {
                return allow();
            }
            return deny("Wrong request");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_user_type_check() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        // User is a Practitioner
        let decision = runtime.evaluate("return isPractitionerUser();", &context);
        assert!(decision.is_allowed());

        // User is not a Patient
        let decision = runtime.evaluate("return isPatientUser();", &context);
        assert!(decision.is_denied());
    }

    #[test]
    fn test_quickjs_timeout() {
        let config = QuickJsConfig {
            timeout_ms: 50, // Short timeout
            ..Default::default()
        };
        let runtime = QuickJsRuntime::new(config).unwrap();
        let context = create_test_context();

        // Infinite loop should trigger timeout or error
        let decision = runtime.evaluate("while(true) {}", &context);
        assert!(decision.is_denied(), "Infinite loop should be denied");
        // Accept either timeout or error code (timeout detection depends on rquickjs internals)
        if let AccessDecision::Deny(reason) = decision {
            assert!(
                reason.code == "script-timeout" || reason.code == "script-error",
                "Expected timeout or error code, got: {}",
                reason.code
            );
        }
    }

    #[test]
    fn test_quickjs_patient_compartment() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let mut context = create_test_context();
        context.environment.patient_context = Some("pat-123".to_string());
        context.resource = Some(ResourceContext {
            id: "obs-1".to_string(),
            resource_type: "Observation".to_string(),
            version_id: None,
            last_updated: None,
            subject: Some("Patient/pat-123".to_string()),
            author: None,
            resource: serde_json::json!({
                "resourceType": "Observation",
                "id": "obs-1",
                "subject": {"reference": "Patient/pat-123"}
            }),
        });

        let script = r#"
            if (inPatientCompartment()) {
                return allow();
            }
            return deny("Not in compartment");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_es2020_features() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        // Optional chaining and nullish coalescing
        let script = r#"
            const patientId = user?.fhirUserId ?? "unknown";
            if (patientId !== "unknown") {
                return allow();
            }
            return deny("Unknown user");
        "#;

        let decision = runtime.evaluate(script, &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_pool_parallel() {
        use std::sync::Arc;

        let runtime = Arc::new(
            QuickJsRuntime::new(QuickJsConfig {
                pool_size: 4,
                ..Default::default()
            })
            .unwrap(),
        );

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let runtime = runtime.clone();
                std::thread::spawn(move || {
                    let context = create_test_context();
                    let decision = runtime.evaluate("return allow();", &context);
                    (i, decision)
                })
            })
            .collect();

        for handle in handles {
            let (i, decision) = handle.join().unwrap();
            assert!(
                decision.is_allowed(),
                "Thread {} failed with decision: {:?}",
                i,
                decision
            );
        }
    }

    #[test]
    fn test_quickjs_script_error() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        // Syntax error
        let decision = runtime.evaluate("return {{{", &context);
        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.code, "script-error");
        }
    }

    #[test]
    fn test_quickjs_null_user() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let mut context = create_test_context();
        context.user = None;

        // Script should handle null user gracefully
        let decision = runtime.evaluate("return user === null;", &context);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_quickjs_stats() {
        let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
        let context = create_test_context();

        assert_eq!(runtime.stats().evaluations, 0);

        runtime.evaluate("return true;", &context);
        runtime.evaluate("return true;", &context);

        assert_eq!(runtime.stats().evaluations, 2);
    }
}
