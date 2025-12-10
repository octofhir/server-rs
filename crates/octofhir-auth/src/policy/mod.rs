//! AccessPolicy engine for fine-grained authorization.
//!
//! This module provides the policy-based access control system:
//!
//! - AccessPolicy resource management
//! - Policy evaluation engine
//! - Scriptable policy rules
//! - Context-aware authorization decisions
//! - Policy caching and optimization
//!
//! # Policy Evaluation Context
//!
//! The [`context`] module provides the [`PolicyContext`] structure that contains
//! all information needed to make access control decisions:
//!
//! ```ignore
//! use octofhir_auth::policy::context::{PolicyContext, PolicyContextBuilder};
//!
//! let context = PolicyContextBuilder::new()
//!     .with_auth_context(&auth)
//!     .with_request("GET", "/Patient/123", query_params, None)
//!     .with_environment("req-abc", Some(source_ip))
//!     .build()?;
//! ```
//!
//! # Pattern Matching
//!
//! The [`matcher`] module provides pattern matching to determine which requests
//! a policy applies to:
//!
//! ```ignore
//! use octofhir_auth::policy::matcher::{PatternMatcher, PolicyMatchers};
//!
//! let matcher = PatternMatcher::new();
//! let matchers = PolicyMatchers {
//!     roles: Some(vec!["doctor".to_string()]),
//!     resource_types: Some(vec!["Patient".to_string()]),
//!     ..Default::default()
//! };
//!
//! if matcher.matches(&matchers, &context) {
//!     // Policy applies to this request
//! }
//! ```
//!
//! # AccessPolicy Resource
//!
//! The [`resources`] module provides the AccessPolicy FHIR-like resource for
//! configuring access control policies:
//!
//! ```ignore
//! use octofhir_auth::policy::resources::{AccessPolicy, EngineElement, PolicyEngineType};
//!
//! let policy = AccessPolicy {
//!     name: "Allow admin reads".to_string(),
//!     engine: EngineElement {
//!         engine_type: PolicyEngineType::Allow,
//!         script: None,
//!     },
//!     ..Default::default()
//! };
//!
//! policy.validate()?;
//! let internal = policy.to_internal_policy()?;
//! ```
//!
//! # Policy Cache
//!
//! The [`cache`] module provides an in-memory cache for active policies with
//! automatic refresh and resource type indexing:
//!
//! ```ignore
//! use octofhir_auth::policy::cache::PolicyCache;
//! use std::sync::Arc;
//! use time::Duration;
//!
//! let cache = PolicyCache::new(storage, Duration::minutes(5));
//!
//! // Get policies applicable to Patient resources
//! let policies = cache.get_applicable_policies("Patient").await?;
//! ```
//!
//! # Policy Evaluation Engine
//!
//! The [`engine`] module provides the policy evaluation engine:
//!
//! ```ignore
//! use octofhir_auth::policy::engine::{PolicyEvaluator, PolicyEvaluatorConfig, DefaultDecision};
//!
//! let evaluator = PolicyEvaluator::new(cache, PolicyEvaluatorConfig {
//!     default_decision: DefaultDecision::Deny,
//!     evaluate_scopes_first: true,
//!     ..Default::default()
//! });
//!
//! let decision = evaluator.evaluate(&context).await;
//! ```
//!
//! [`PolicyContext`]: context::PolicyContext

pub mod cache;
pub mod compartment;
pub mod context;
pub mod engine;
pub mod matcher;
pub mod quickjs;
pub mod reload;
pub mod resources;

pub use cache::{PolicyCache, PolicyCacheError, PolicyCacheStats};

pub use engine::{
    AccessDecision, DefaultDecision, DenyReason, EvaluatedPolicy, EvaluationResult,
    PolicyEvaluator, PolicyEvaluatorConfig,
};

pub use context::{
    ClientIdentity, ClientType, ContextError, EnvironmentContext, PolicyContext,
    PolicyContextBuilder, RequestContext, ResourceContext, ScopeSummary, UserIdentity,
    detect_operation, parse_fhir_path,
};

pub use matcher::{
    CompartmentIdSource, CompartmentMatcher, MatchPattern, PatternMatcher, PolicyMatchers,
};

pub use resources::{
    AccessPolicy, ConversionError, EngineElement, InternalPolicy, MatcherElement, PolicyEngine,
    PolicyEngineType, ResourceMeta, ValidationError,
};

pub use compartment::{
    CompartmentChecker, CompartmentDefinition, CompartmentInclusion, PatientCompartmentPolicy,
};

pub use quickjs::{QuickJsCacheStats, QuickJsError, QuickJsRuntime};

pub use reload::{
    PolicyChange, PolicyChangeNotifier, PolicyReloadService, ReloadConfig, ReloadStats,
};
