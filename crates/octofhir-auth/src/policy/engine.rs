//! Policy evaluation engine for access control decisions.
//!
//! This module provides the core policy evaluation engine that orchestrates
//! policy lookup, matching, and decision-making.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::policy::engine::{PolicyEvaluator, PolicyEvaluatorConfig, DefaultDecision};
//! use std::sync::Arc;
//!
//! let evaluator = PolicyEvaluator::new(policy_cache, PolicyEvaluatorConfig {
//!     quickjs_enabled: false,
//!     default_decision: DefaultDecision::Deny,
//!     evaluate_scopes_first: true,
//!     ..Default::default()
//! });
//!
//! let decision = evaluator.evaluate(&context).await;
//! if decision.is_allowed() {
//!     // Proceed with request
//! }
//! ```

use std::sync::Arc;

use serde::Serialize;

use crate::AuthResult;
use crate::config::QuickJsConfig;
use crate::policy::cache::PolicyCache;
use crate::policy::context::PolicyContext;
use crate::policy::matcher::PatternMatcher;
use crate::policy::quickjs::QuickJsRuntime;
use crate::policy::resources::{InternalPolicy, PolicyEngine as PolicyEngineType};
use crate::smart::scopes::{FhirOperation, SmartScopes};

// =============================================================================
// Access Decision
// =============================================================================

/// Result of policy evaluation.
#[derive(Debug, Clone)]
pub enum AccessDecision {
    /// Access is granted.
    Allow,
    /// Access is denied with a reason.
    Deny(DenyReason),
    /// Cannot make a decision, continue to next policy.
    Abstain,
}

impl AccessDecision {
    /// Returns `true` if access was granted.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Returns `true` if access was denied.
    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny(_))
    }

    /// Returns `true` if the policy abstained from making a decision.
    #[must_use]
    pub fn is_abstain(&self) -> bool {
        matches!(self, Self::Abstain)
    }

    /// Get the deny reason if access was denied.
    #[must_use]
    pub fn deny_reason(&self) -> Option<&DenyReason> {
        match self {
            Self::Deny(reason) => Some(reason),
            _ => None,
        }
    }
}

// =============================================================================
// Deny Reason
// =============================================================================

/// Reason for access denial.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenyReason {
    /// Error code for programmatic handling.
    pub code: String,

    /// Human-readable error message.
    pub message: String,

    /// Additional details about the denial.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,

    /// ID of the policy that denied access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_id: Option<String>,
}

impl DenyReason {
    /// Create a denial reason for no matching policy.
    #[must_use]
    pub fn no_matching_policy() -> Self {
        Self {
            code: "no-matching-policy".to_string(),
            message: "No policy granted access to this resource".to_string(),
            details: None,
            policy_id: None,
        }
    }

    /// Create a denial reason from a policy decision.
    #[must_use]
    pub fn policy_denied(policy_id: &str, message: Option<String>) -> Self {
        Self {
            code: "policy-denied".to_string(),
            message: message.unwrap_or_else(|| "Access denied by policy".to_string()),
            details: None,
            policy_id: Some(policy_id.to_string()),
        }
    }

    /// Create a denial reason for insufficient scope.
    #[must_use]
    pub fn scope_insufficient(required: &str) -> Self {
        Self {
            code: "insufficient-scope".to_string(),
            message: format!(
                "Token scope does not include required permission: {}",
                required
            ),
            details: Some(serde_json::json!({ "required_scope": required })),
            policy_id: None,
        }
    }

    /// Create a denial reason for a policy error.
    #[must_use]
    pub fn policy_error(message: impl Into<String>) -> Self {
        Self {
            code: "policy-error".to_string(),
            message: message.into(),
            details: None,
            policy_id: None,
        }
    }

    /// Create a denial reason for script execution failure.
    #[must_use]
    pub fn script_error(policy_id: &str, error: impl Into<String>) -> Self {
        Self {
            code: "script-error".to_string(),
            message: format!("Policy script failed: {}", error.into()),
            details: None,
            policy_id: Some(policy_id.to_string()),
        }
    }
}

// =============================================================================
// Policy Engine Configuration
// =============================================================================

/// Configuration for the policy evaluator.
#[derive(Debug, Clone)]
pub struct PolicyEvaluatorConfig {
    /// Enable QuickJS script engine for policies.
    pub quickjs_enabled: bool,

    /// QuickJS engine configuration.
    pub quickjs_config: QuickJsConfig,

    /// Default decision when no policy matches.
    pub default_decision: DefaultDecision,

    /// Evaluate SMART scopes before policies.
    ///
    /// If enabled, requests are first checked against SMART scopes.
    /// Scope denial short-circuits policy evaluation.
    pub evaluate_scopes_first: bool,
}

impl Default for PolicyEvaluatorConfig {
    fn default() -> Self {
        Self {
            quickjs_enabled: false,
            quickjs_config: QuickJsConfig::default(),
            default_decision: DefaultDecision::Deny,
            evaluate_scopes_first: true,
        }
    }
}

/// Default decision when no policy matches the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultDecision {
    /// Allow access if no policy denies it.
    Allow,
    /// Deny access unless a policy explicitly allows it.
    Deny,
}

// =============================================================================
// Evaluation Result
// =============================================================================

/// Complete result of policy evaluation with audit information.
#[derive(Debug)]
pub struct EvaluationResult {
    /// The final access decision.
    pub decision: AccessDecision,

    /// Policies that were evaluated.
    pub evaluated_policies: Vec<EvaluatedPolicy>,

    /// Time taken to evaluate policies (milliseconds).
    pub evaluation_time_ms: f64,

    /// Whether scopes were checked.
    pub scopes_checked: bool,

    /// Scope check result (if checked).
    pub scope_decision: Option<AccessDecision>,
}

/// Information about a policy that was evaluated.
#[derive(Debug, Clone)]
pub struct EvaluatedPolicy {
    /// Policy ID.
    pub policy_id: String,

    /// Policy name.
    pub policy_name: String,

    /// Whether the policy's matchers matched the context.
    pub matched: bool,

    /// The decision from this policy (if it was evaluated).
    pub decision: Option<AccessDecision>,
}

// =============================================================================
// Policy Evaluator
// =============================================================================

/// Policy evaluation engine.
///
/// Orchestrates policy lookup, matching, and decision-making.
///
/// QuickJS runtime is created once and shared across all evaluations.
/// Scripts are compiled to AST once and cached.
pub struct PolicyEvaluator {
    /// Pattern matcher for policy matching.
    pattern_matcher: PatternMatcher,

    /// Policy cache for efficient policy lookup.
    policy_cache: Arc<PolicyCache>,

    /// QuickJS scripting runtime (shared pool, created once).
    quickjs_runtime: Option<Arc<QuickJsRuntime>>,

    /// Engine configuration.
    config: PolicyEvaluatorConfig,
}

impl PolicyEvaluator {
    /// Create a new policy evaluator.
    ///
    /// If `quickjs_enabled` is true, a shared QuickJS runtime pool is created.
    /// The runtime is reused for all script evaluations.
    #[must_use]
    pub fn new(policy_cache: Arc<PolicyCache>, config: PolicyEvaluatorConfig) -> Self {
        let quickjs_runtime = if config.quickjs_enabled {
            match QuickJsRuntime::new(config.quickjs_config.clone()) {
                Ok(runtime) => Some(Arc::new(runtime)),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to initialize QuickJS runtime");
                    None
                }
            }
        } else {
            None
        };

        Self {
            pattern_matcher: PatternMatcher::new(),
            policy_cache,
            quickjs_runtime,
            config,
        }
    }

    /// Evaluate access for a request context.
    ///
    /// # Evaluation Order
    ///
    /// 1. Check SMART scopes (if `evaluate_scopes_first` is enabled)
    /// 2. Get applicable policies from cache
    /// 3. Evaluate policies in priority order
    /// 4. Return first deny or allow; use default if no match
    pub async fn evaluate(&self, context: &PolicyContext) -> AccessDecision {
        // Step 1: Check SMART scopes first (if enabled)
        if self.config.evaluate_scopes_first
            && let Some(decision) = self.check_smart_scopes(context)
            && decision.is_denied()
        {
            return decision;
        }

        // Step 2: Get applicable policies
        let policies = match self
            .policy_cache
            .get_applicable_policies(&context.request.resource_type)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to get policies from cache");
                return AccessDecision::Deny(DenyReason::policy_error(
                    "Failed to evaluate access policies",
                ));
            }
        };

        // Step 3: Evaluate policies in priority order
        let mut has_allow = false;

        for policy in &policies {
            // Check if policy matches this context
            if !self.pattern_matcher.matches(&policy.matchers, context) {
                continue;
            }

            // Evaluate policy engine
            let decision = self.evaluate_policy_engine(policy, context);

            match decision {
                AccessDecision::Deny(reason) => {
                    // First deny wins
                    tracing::debug!(
                        policy_id = %policy.id,
                        policy_name = %policy.name,
                        reason = %reason.message,
                        "Policy denied access"
                    );
                    return AccessDecision::Deny(reason);
                }
                AccessDecision::Allow => {
                    tracing::debug!(
                        policy_id = %policy.id,
                        policy_name = %policy.name,
                        "Policy allowed access"
                    );
                    has_allow = true;
                }
                AccessDecision::Abstain => {
                    tracing::trace!(
                        policy_id = %policy.id,
                        policy_name = %policy.name,
                        "Policy abstained"
                    );
                }
            }
        }

        // Step 4: Return result
        if has_allow {
            AccessDecision::Allow
        } else {
            match self.config.default_decision {
                DefaultDecision::Allow => AccessDecision::Allow,
                DefaultDecision::Deny => AccessDecision::Deny(DenyReason::no_matching_policy()),
            }
        }
    }

    /// Evaluate access with detailed audit information.
    pub async fn evaluate_with_audit(&self, context: &PolicyContext) -> EvaluationResult {
        let start = std::time::Instant::now();
        let mut evaluated_policies = Vec::new();
        let mut scope_decision = None;
        let scopes_checked = self.config.evaluate_scopes_first;

        // Step 1: Check SMART scopes first (if enabled)
        if self.config.evaluate_scopes_first
            && let Some(decision) = self.check_smart_scopes(context)
        {
            scope_decision = Some(decision.clone());
            if decision.is_denied() {
                return EvaluationResult {
                    decision,
                    evaluated_policies,
                    evaluation_time_ms: start.elapsed().as_secs_f64() * 1000.0,
                    scopes_checked,
                    scope_decision,
                };
            }
        }

        // Step 2: Get applicable policies
        let policies = match self
            .policy_cache
            .get_applicable_policies(&context.request.resource_type)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to get policies from cache");
                return EvaluationResult {
                    decision: AccessDecision::Deny(DenyReason::policy_error(
                        "Failed to evaluate access policies",
                    )),
                    evaluated_policies,
                    evaluation_time_ms: start.elapsed().as_secs_f64() * 1000.0,
                    scopes_checked,
                    scope_decision,
                };
            }
        };

        // Step 3: Evaluate policies in priority order
        let mut has_allow = false;
        let mut final_decision = None;

        for policy in &policies {
            // Check if policy matches this context
            let matched = self.pattern_matcher.matches(&policy.matchers, context);

            if !matched {
                evaluated_policies.push(EvaluatedPolicy {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    matched: false,
                    decision: None,
                });
                continue;
            }

            // Evaluate policy engine
            let decision = self.evaluate_policy_engine(policy, context);

            evaluated_policies.push(EvaluatedPolicy {
                policy_id: policy.id.clone(),
                policy_name: policy.name.clone(),
                matched: true,
                decision: Some(decision.clone()),
            });

            match decision {
                AccessDecision::Deny(_) => {
                    final_decision = Some(decision);
                    break;
                }
                AccessDecision::Allow => {
                    has_allow = true;
                }
                AccessDecision::Abstain => {}
            }
        }

        // Step 4: Determine final decision
        let decision = final_decision.unwrap_or_else(|| {
            if has_allow {
                AccessDecision::Allow
            } else {
                match self.config.default_decision {
                    DefaultDecision::Allow => AccessDecision::Allow,
                    DefaultDecision::Deny => AccessDecision::Deny(DenyReason::no_matching_policy()),
                }
            }
        });

        EvaluationResult {
            decision,
            evaluated_policies,
            evaluation_time_ms: start.elapsed().as_secs_f64() * 1000.0,
            scopes_checked,
            scope_decision,
        }
    }

    /// Check SMART scopes against the operation.
    fn check_smart_scopes(&self, context: &PolicyContext) -> Option<AccessDecision> {
        let resource_type = &context.request.resource_type;
        let operation = context.request.operation;

        // Parse the raw scope string to check permissions
        let scopes = SmartScopes::parse(&context.scopes.raw).unwrap_or_default();

        // Operations that don't require scope checking
        if operation.always_allowed() {
            return None;
        }

        // Determine required permission based on operation
        let required_permission = match operation {
            FhirOperation::Read
            | FhirOperation::VRead
            | FhirOperation::Search
            | FhirOperation::SearchType
            | FhirOperation::SearchSystem
            | FhirOperation::HistoryInstance
            | FhirOperation::HistoryType
            | FhirOperation::HistorySystem => "r",
            FhirOperation::Create => "c",
            FhirOperation::Update | FhirOperation::Patch => "u",
            FhirOperation::Delete => "d",
            // System operations don't require scope checking
            FhirOperation::Capabilities
            | FhirOperation::Batch
            | FhirOperation::Transaction
            | FhirOperation::Operation => return None,
        };

        // Check if any scope permits this operation
        let patient_context = context.environment.patient_context.as_deref();

        if scopes.permits(resource_type, operation, patient_context) {
            Some(AccessDecision::Allow)
        } else {
            let required_scope = format!(
                "patient/{}.{} or user/{}.{}",
                resource_type, required_permission, resource_type, required_permission
            );
            Some(AccessDecision::Deny(DenyReason::scope_insufficient(
                &required_scope,
            )))
        }
    }

    /// Evaluate a single policy's engine.
    fn evaluate_policy_engine(
        &self,
        policy: &InternalPolicy,
        context: &PolicyContext,
    ) -> AccessDecision {
        match &policy.engine {
            PolicyEngineType::Allow => AccessDecision::Allow,

            PolicyEngineType::Deny => AccessDecision::Deny(DenyReason::policy_denied(
                &policy.id,
                policy.deny_message.clone(),
            )),

            PolicyEngineType::QuickJs { script } => {
                if let Some(ref runtime) = self.quickjs_runtime {
                    tracing::debug!(
                        policy_id = %policy.id,
                        "Evaluating QuickJS policy script"
                    );
                    let decision = runtime.evaluate(script, context);
                    // If the script returns a deny, attach the policy ID
                    if let AccessDecision::Deny(mut reason) = decision {
                        reason.policy_id = Some(policy.id.clone());
                        AccessDecision::Deny(reason)
                    } else {
                        decision
                    }
                } else {
                    tracing::warn!(
                        policy_id = %policy.id,
                        "QuickJS policy but QuickJS is disabled"
                    );
                    AccessDecision::Abstain
                }
            }
        }
    }

    /// Get a reference to the policy cache.
    #[must_use]
    pub fn cache(&self) -> &PolicyCache {
        &self.policy_cache
    }

    /// Get a reference to the QuickJS runtime (if enabled).
    #[must_use]
    pub fn quickjs_runtime(&self) -> Option<&Arc<QuickJsRuntime>> {
        self.quickjs_runtime.as_ref()
    }

    /// Invalidate the policy cache.
    pub async fn invalidate_cache(&self) {
        self.policy_cache.invalidate().await;
    }

    /// Force refresh the policy cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache refresh fails.
    pub async fn refresh_cache(&self) -> AuthResult<()> {
        self.policy_cache.refresh().await
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::resources::{
        AccessPolicy, EngineElement, MatcherElement, PolicyEngineType as ResourcePolicyEngineType,
    };
    use crate::storage::PolicyStorage;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use time::Duration;

    // -------------------------------------------------------------------------
    // Mock Storage
    // -------------------------------------------------------------------------

    struct MockPolicyStorage {
        policies: std::sync::RwLock<Vec<AccessPolicy>>,
        call_count: AtomicUsize,
    }

    impl MockPolicyStorage {
        fn new() -> Self {
            Self {
                policies: std::sync::RwLock::new(Vec::new()),
                call_count: AtomicUsize::new(0),
            }
        }

        fn add_policy(&self, policy: AccessPolicy) {
            self.policies.write().unwrap().push(policy);
        }
    }

    #[async_trait]
    impl PolicyStorage for MockPolicyStorage {
        async fn get(&self, id: &str) -> AuthResult<Option<AccessPolicy>> {
            Ok(self
                .policies
                .read()
                .unwrap()
                .iter()
                .find(|p| p.id.as_deref() == Some(id))
                .cloned())
        }

        async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let policies = self.policies.read().unwrap();
            let mut active: Vec<_> = policies.iter().filter(|p| p.active).cloned().collect();
            active.sort_by_key(|p| p.priority);
            Ok(active)
        }

        async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>> {
            Ok(self.policies.read().unwrap().clone())
        }

        async fn create(&self, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn update(&self, _id: &str, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn delete(&self, _id: &str) -> AuthResult<()> {
            unimplemented!()
        }

        async fn find_applicable(
            &self,
            _resource_type: &str,
            _operation: FhirOperation,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn get_by_ids(&self, _ids: &[String]) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn search(
            &self,
            _params: &crate::storage::PolicySearchParams,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_client(&self, _client_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_user(&self, _user_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_role(&self, _role: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn upsert(&self, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }
    }

    // -------------------------------------------------------------------------
    // Helper Functions
    // -------------------------------------------------------------------------

    fn create_test_context(resource_type: &str, operation: FhirOperation) -> PolicyContext {
        use crate::policy::context::{
            ClientIdentity, ClientType, EnvironmentContext, RequestContext, ScopeSummary,
        };
        use time::OffsetDateTime;

        PolicyContext {
            user: None,
            client: ClientIdentity {
                id: "test-client".to_string(),
                name: "Test Client".to_string(),
                trusted: false,
                client_type: ClientType::Public,
            },
            scopes: ScopeSummary {
                raw: format!("user/{}.cruds", resource_type),
                patient_scopes: vec![],
                user_scopes: vec![format!("user/{}.cruds", resource_type)],
                system_scopes: vec![],
                has_wildcard: false,
                launch: false,
                openid: false,
                fhir_user: false,
                offline_access: false,
            },
            request: RequestContext {
                operation,
                resource_type: resource_type.to_string(),
                resource_id: None,
                compartment_type: None,
                compartment_id: None,
                body: None,
                query_params: std::collections::HashMap::new(),
                path: format!("/{}", resource_type),
                method: "GET".to_string(),
                operation_id: None,
            },
            resource: None,
            environment: EnvironmentContext {
                request_time: OffsetDateTime::now_utc(),
                source_ip: None,
                request_id: "test-request".to_string(),
                patient_context: None,
                encounter_context: None,
            },
        }
    }

    async fn create_test_engine(storage: Arc<MockPolicyStorage>) -> PolicyEvaluator {
        let cache = Arc::new(PolicyCache::new(storage, Duration::minutes(5)));
        cache.refresh().await.unwrap();

        PolicyEvaluator::new(
            cache,
            PolicyEvaluatorConfig {
                quickjs_enabled: false,
                default_decision: DefaultDecision::Deny,
                evaluate_scopes_first: false, // Disable for simpler testing
                ..Default::default()
            },
        )
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_allow_policy() {
        let storage = Arc::new(MockPolicyStorage::new());
        storage.add_policy(AccessPolicy {
            id: Some("allow-all".to_string()),
            name: "Allow all".to_string(),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_deny_policy() {
        let storage = Arc::new(MockPolicyStorage::new());
        storage.add_policy(AccessPolicy {
            id: Some("deny-all".to_string()),
            name: "Deny all".to_string(),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Deny,
                script: None,
            },
            deny_message: Some("Access denied".to_string()),
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.message, "Access denied");
            assert_eq!(reason.policy_id, Some("deny-all".to_string()));
        }
    }

    #[tokio::test]
    async fn test_first_deny_wins() {
        let storage = Arc::new(MockPolicyStorage::new());

        // Add allow policy (priority 100 - evaluated second)
        storage.add_policy(AccessPolicy {
            id: Some("allow".to_string()),
            name: "Allow".to_string(),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        });

        // Add deny policy (priority 50 - evaluated first)
        storage.add_policy(AccessPolicy {
            id: Some("deny".to_string()),
            name: "Deny".to_string(),
            active: true,
            priority: 50,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Deny,
                script: None,
            },
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        // Deny should win because it's evaluated first (lower priority number)
        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_policy_not_matching_skipped() {
        let storage = Arc::new(MockPolicyStorage::new());

        // Policy only for Observation
        storage.add_policy(AccessPolicy {
            id: Some("observation-only".to_string()),
            name: "Observation only".to_string(),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                resource_types: Some(vec!["Observation".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;

        // Request for Patient should not match
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        // Default deny (no matching policy)
        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.code, "no-matching-policy");
        }
    }

    #[tokio::test]
    async fn test_no_policy_uses_default_deny() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, Duration::minutes(5)));
        cache.refresh().await.unwrap();

        let engine = PolicyEvaluator::new(
            cache,
            PolicyEvaluatorConfig {
                quickjs_enabled: false,
                default_decision: DefaultDecision::Deny,
                evaluate_scopes_first: false,
                ..Default::default()
            },
        );

        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn test_no_policy_uses_default_allow() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, Duration::minutes(5)));
        cache.refresh().await.unwrap();

        let engine = PolicyEvaluator::new(
            cache,
            PolicyEvaluatorConfig {
                quickjs_enabled: false,
                default_decision: DefaultDecision::Allow,
                evaluate_scopes_first: false,
                ..Default::default()
            },
        );

        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_smart_scope_enforcement_allows() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, Duration::minutes(5)));
        cache.refresh().await.unwrap();

        let engine = PolicyEvaluator::new(
            cache,
            PolicyEvaluatorConfig {
                quickjs_enabled: false,
                default_decision: DefaultDecision::Deny,
                evaluate_scopes_first: true,
                ..Default::default()
            },
        );

        // Context with Patient read scope
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        // Scope check passes, but no policy matches so default deny
        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.code, "no-matching-policy");
        }
    }

    #[tokio::test]
    async fn test_smart_scope_enforcement_denies() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, Duration::minutes(5)));
        cache.refresh().await.unwrap();

        let engine = PolicyEvaluator::new(
            cache,
            PolicyEvaluatorConfig {
                quickjs_enabled: false,
                default_decision: DefaultDecision::Allow,
                evaluate_scopes_first: true,
                ..Default::default()
            },
        );

        // Context with Patient scope but requesting Observation
        let mut context = create_test_context("Patient", FhirOperation::Read);
        context.request.resource_type = "Observation".to_string();
        // Scope is still user/Patient.cruds, not Observation

        let decision = engine.evaluate(&context).await;

        // Should deny - scope doesn't cover Observation
        assert!(decision.is_denied());
        if let AccessDecision::Deny(reason) = decision {
            assert_eq!(reason.code, "insufficient-scope");
        }
    }

    #[tokio::test]
    async fn test_evaluate_with_audit() {
        let storage = Arc::new(MockPolicyStorage::new());
        // Policy that only allows doctors
        storage.add_policy(AccessPolicy {
            id: Some("p1".to_string()),
            name: "Doctors only".to_string(),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                roles: Some(vec!["doctor".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        });
        // Policy that allows all Patient resources
        storage.add_policy(AccessPolicy {
            id: Some("p2".to_string()),
            name: "Allow Patient".to_string(),
            active: true,
            priority: 200,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                resource_types: Some(vec!["Patient".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;
        let context = create_test_context("Patient", FhirOperation::Read);
        let result = engine.evaluate_with_audit(&context).await;

        // Should have evaluated both policies (both match Patient via wildcard or specific)
        assert_eq!(result.evaluated_policies.len(), 2);

        // First policy (doctors only) should match Patient but user has no roles
        // so the matcher should not match
        assert!(!result.evaluated_policies[0].matched);
        assert!(result.evaluated_policies[0].decision.is_none());

        // Second policy should match and allow
        assert!(result.evaluated_policies[1].matched);
        assert!(
            result.evaluated_policies[1]
                .decision
                .as_ref()
                .unwrap()
                .is_allowed()
        );

        // Final decision should be allow
        assert!(result.decision.is_allowed());
        assert!(result.evaluation_time_ms >= 0.0);
    }

    #[tokio::test]
    async fn test_deny_reason_types() {
        let no_match = DenyReason::no_matching_policy();
        assert_eq!(no_match.code, "no-matching-policy");

        let policy_denied = DenyReason::policy_denied("p1", Some("Custom message".to_string()));
        assert_eq!(policy_denied.code, "policy-denied");
        assert_eq!(policy_denied.message, "Custom message");
        assert_eq!(policy_denied.policy_id, Some("p1".to_string()));

        let scope = DenyReason::scope_insufficient("patient/Patient.r");
        assert_eq!(scope.code, "insufficient-scope");
        assert!(scope.details.is_some());

        let error = DenyReason::policy_error("Something went wrong");
        assert_eq!(error.code, "policy-error");

        let script = DenyReason::script_error("p1", "Syntax error");
        assert_eq!(script.code, "script-error");
    }

    #[tokio::test]
    async fn test_access_decision_methods() {
        let allow = AccessDecision::Allow;
        assert!(allow.is_allowed());
        assert!(!allow.is_denied());
        assert!(!allow.is_abstain());
        assert!(allow.deny_reason().is_none());

        let deny = AccessDecision::Deny(DenyReason::no_matching_policy());
        assert!(!deny.is_allowed());
        assert!(deny.is_denied());
        assert!(!deny.is_abstain());
        assert!(deny.deny_reason().is_some());

        let abstain = AccessDecision::Abstain;
        assert!(!abstain.is_allowed());
        assert!(!abstain.is_denied());
        assert!(abstain.is_abstain());
    }

    #[tokio::test]
    async fn test_inactive_policy_ignored() {
        let storage = Arc::new(MockPolicyStorage::new());
        storage.add_policy(AccessPolicy {
            id: Some("inactive".to_string()),
            name: "Inactive policy".to_string(),
            active: false, // Inactive
            priority: 1,
            engine: EngineElement {
                engine_type: ResourcePolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        });

        let engine = create_test_engine(storage).await;
        let context = create_test_context("Patient", FhirOperation::Read);
        let decision = engine.evaluate(&context).await;

        // Inactive policy should be ignored, default deny
        assert!(decision.is_denied());
    }
}
