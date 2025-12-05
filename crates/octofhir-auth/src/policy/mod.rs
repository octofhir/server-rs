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
//! [`PolicyContext`]: context::PolicyContext

pub mod context;
pub mod matcher;
pub mod resources;

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
