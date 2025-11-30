//! SMART on FHIR implementation.
//!
//! This module provides SMART on FHIR specific functionality:
//!
//! - SMART scopes parsing and validation
//! - Launch context handling (EHR launch, standalone launch)
//! - SMART configuration endpoint (/.well-known/smart-configuration)
//! - CapabilityStatement security extensions
//! - Clinical scope enforcement
//! - Patient context selection

pub mod conformance;
pub mod discovery;
pub mod launch;
pub mod scopes;

pub use conformance::{CapabilitySecurityBuilder, ConformanceError, add_smart_security};
pub use discovery::SmartConfiguration;
pub use launch::{
    DEFAULT_LAUNCH_CONTEXT_TTL, FhirContextItem, StoredLaunchContext, generate_launch_id,
};
pub use scopes::{
    FhirOperation, Permissions, ResourceType, ScopeContext, ScopeError, ScopeFilter, SmartScope,
    SmartScopes, StandaloneContextRequirements,
};
