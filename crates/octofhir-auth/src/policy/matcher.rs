//! Pattern matching for policy evaluation.
//!
//! This module provides the pattern matching component that determines which
//! requests a policy applies to based on various criteria including client ID,
//! user roles, resource types, FHIR operations, IP addresses, and compartments.
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_auth::policy::matcher::{PatternMatcher, PolicyMatchers, MatchPattern};
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

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;

use ipnetwork::IpNetwork;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::policy::context::PolicyContext;
use crate::smart::scopes::FhirOperation;

// =============================================================================
// Policy Matchers
// =============================================================================

/// Matchers determine which requests a policy applies to.
///
/// All specified matchers must match (AND logic within a policy).
/// A matcher field set to `None` is not evaluated (matches any value).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyMatchers {
    /// Match by client ID patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<MatchPattern>>,

    /// Match by user roles (any role matches).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,

    /// Match by user FHIR resource type (e.g., "Practitioner", "Patient").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_types: Option<Vec<String>>,

    /// Match by FHIR resource type being accessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_types: Option<Vec<String>>,

    /// Match by FHIR operation being performed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<FhirOperation>>,

    /// Match by operation ID (e.g., "fhir.read", "graphql.query").
    /// Supports wildcards with `*` (e.g., "fhir.*", "ui.admin.*").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_ids: Option<Vec<String>>,

    /// Match by compartment membership.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compartments: Option<Vec<CompartmentMatcher>>,

    /// Match by request path pattern (glob syntax).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,

    /// Match by source IP address (CIDR notation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ips: Option<Vec<String>>,

    /// Require specific scopes to be present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_scopes: Option<Vec<String>>,
}

// =============================================================================
// Match Pattern
// =============================================================================

/// Pattern matching options for string values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MatchPattern {
    /// Exact string match.
    Exact {
        /// The exact value to match.
        value: String,
    },
    /// Prefix match.
    Prefix {
        /// The prefix to match.
        value: String,
    },
    /// Suffix match.
    Suffix {
        /// The suffix to match.
        value: String,
    },
    /// Regular expression match.
    Regex {
        /// The regex pattern.
        pattern: String,
    },
    /// Wildcard (matches anything).
    Wildcard,
}

impl MatchPattern {
    /// Check if the pattern matches a value.
    #[must_use]
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact { value: pattern } => value == pattern,
            Self::Prefix { value: pattern } => value.starts_with(pattern),
            Self::Suffix { value: pattern } => value.ends_with(pattern),
            Self::Regex { pattern } => Regex::new(pattern)
                .map(|re| re.is_match(value))
                .unwrap_or(false),
            Self::Wildcard => true,
        }
    }

    /// Create an exact match pattern.
    #[must_use]
    pub fn exact(value: impl Into<String>) -> Self {
        Self::Exact {
            value: value.into(),
        }
    }

    /// Create a prefix match pattern.
    #[must_use]
    pub fn prefix(value: impl Into<String>) -> Self {
        Self::Prefix {
            value: value.into(),
        }
    }

    /// Create a suffix match pattern.
    #[must_use]
    pub fn suffix(value: impl Into<String>) -> Self {
        Self::Suffix {
            value: value.into(),
        }
    }

    /// Create a regex match pattern.
    #[must_use]
    pub fn regex(pattern: impl Into<String>) -> Self {
        Self::Regex {
            pattern: pattern.into(),
        }
    }

    /// Create a wildcard pattern.
    #[must_use]
    pub fn wildcard() -> Self {
        Self::Wildcard
    }
}

// =============================================================================
// Compartment Matcher
// =============================================================================

/// Compartment-based access control matcher.
///
/// Verifies that the resource being accessed is within the specified compartment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompartmentMatcher {
    /// Compartment type (e.g., "Patient", "Practitioner").
    pub compartment_type: String,

    /// Source of the compartment ID to match against.
    pub compartment_id: CompartmentIdSource,
}

/// Source of the compartment ID for matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase")]
pub enum CompartmentIdSource {
    /// From SMART launch context (patient or encounter).
    LaunchContext,

    /// From the user's FHIR resource reference.
    UserFhirResource,

    /// A fixed/static value.
    Fixed {
        /// The fixed compartment ID.
        value: String,
    },

    /// From a request query parameter.
    RequestParam {
        /// The parameter name.
        param: String,
    },

    /// From a token claim.
    TokenClaim {
        /// The claim name.
        claim: String,
    },
}

// =============================================================================
// Pattern Matcher
// =============================================================================

/// Pattern matcher with regex caching for performance.
///
/// This struct is thread-safe and can be shared across requests.
pub struct PatternMatcher {
    /// Cache for compiled regex patterns.
    regex_cache: RwLock<HashMap<String, Regex>>,
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternMatcher {
    /// Create a new pattern matcher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            regex_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Check if all matchers match the given context.
    ///
    /// Returns `true` if ALL specified matchers match (AND logic).
    /// Matchers set to `None` are not evaluated.
    #[must_use]
    pub fn matches(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        self.matches_clients(matchers, context)
            && self.matches_roles(matchers, context)
            && self.matches_user_types(matchers, context)
            && self.matches_resource_types(matchers, context)
            && self.matches_operations(matchers, context)
            && self.matches_operation_ids(matchers, context)
            && self.matches_paths(matchers, context)
            && self.matches_source_ips(matchers, context)
            && self.matches_compartments(matchers, context)
            && self.matches_required_scopes(matchers, context)
    }

    fn matches_clients(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers
            .clients
            .as_ref()
            .is_none_or(|clients| self.matches_any_pattern(clients, &context.client.id))
    }

    fn matches_roles(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        let Some(ref roles) = matchers.roles else {
            return true;
        };
        context
            .user
            .as_ref()
            .is_some_and(|user| roles.iter().any(|r| user.roles.contains(r)))
    }

    fn matches_user_types(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        let Some(ref user_types) = matchers.user_types else {
            return true;
        };
        context
            .user
            .as_ref()
            .and_then(|u| u.fhir_user_type.as_ref())
            .is_some_and(|fhir_type| user_types.contains(fhir_type))
    }

    fn matches_resource_types(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers
            .resource_types
            .as_ref()
            .is_none_or(|resource_types| {
                resource_types.contains(&context.request.resource_type)
                    || resource_types.iter().any(|t| t == "*")
            })
    }

    fn matches_operations(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers
            .operations
            .as_ref()
            .is_none_or(|ops| ops.contains(&context.request.operation))
    }

    fn matches_operation_ids(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        let Some(ref patterns) = matchers.operation_ids else {
            return true;
        };
        let Some(ref operation_id) = context.request.operation_id else {
            // If no operation_id in context but patterns are specified, no match
            return false;
        };
        patterns.iter().any(|pattern| self.matches_operation_id_pattern(pattern, operation_id))
    }

    /// Match an operation ID against a pattern.
    ///
    /// Supports wildcards:
    /// - `*` at the end matches any suffix (e.g., "fhir.*" matches "fhir.read", "fhir.create")
    /// - Exact match otherwise
    fn matches_operation_id_pattern(&self, pattern: &str, operation_id: &str) -> bool {
        if pattern.ends_with(".*") {
            // Prefix match: "fhir.*" matches "fhir.read", "fhir.create", etc.
            let prefix = &pattern[..pattern.len() - 1]; // Remove the "*"
            operation_id.starts_with(prefix)
        } else if pattern == "*" {
            // Full wildcard matches anything
            true
        } else {
            // Exact match
            pattern == operation_id
        }
    }

    fn matches_paths(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers.paths.as_ref().is_none_or(|paths| {
            paths
                .iter()
                .any(|pattern| self.matches_glob(pattern, &context.request.path))
        })
    }

    fn matches_source_ips(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        let Some(ref source_ips) = matchers.source_ips else {
            return true;
        };
        context
            .environment
            .source_ip
            .as_ref()
            .is_some_and(|ip| source_ips.iter().any(|cidr| self.matches_ip_cidr(cidr, ip)))
    }

    fn matches_compartments(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers.compartments.as_ref().is_none_or(|compartments| {
            compartments
                .iter()
                .all(|c| self.matches_compartment(c, context))
        })
    }

    fn matches_required_scopes(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        matchers.required_scopes.as_ref().is_none_or(|scopes| {
            scopes
                .iter()
                .all(|s| self.scope_matches(&context.scopes.raw, s))
        })
    }

    /// Check if any pattern in the list matches the value.
    fn matches_any_pattern(&self, patterns: &[MatchPattern], value: &str) -> bool {
        patterns.iter().any(|p| p.matches(value))
    }

    /// Match a glob pattern against a path.
    ///
    /// Supports:
    /// - `*` - matches any characters except `/`
    /// - `**` - matches any characters including `/`
    /// - `?` - matches a single character
    fn matches_glob(&self, pattern: &str, path: &str) -> bool {
        // Convert glob to regex
        // Order matters: handle ** before * to avoid double replacement
        let regex_pattern = pattern
            .replace("**", "\x00") // Temporary placeholder
            .replace('*', "[^/]*")
            .replace('\x00', ".*")
            .replace('?', ".");

        let regex_pattern = format!("^{}$", regex_pattern);

        self.get_or_compile_regex(&regex_pattern)
            .map(|re| re.is_match(path))
            .unwrap_or(false)
    }

    /// Check if an IP address matches a CIDR range.
    fn matches_ip_cidr(&self, cidr: &str, ip: &IpAddr) -> bool {
        cidr.parse::<IpNetwork>()
            .map(|network| network.contains(*ip))
            .unwrap_or(false)
    }

    /// Check if a compartment matcher matches the context.
    fn matches_compartment(&self, matcher: &CompartmentMatcher, context: &PolicyContext) -> bool {
        // Get the compartment ID to check against
        let compartment_id = match &matcher.compartment_id {
            CompartmentIdSource::LaunchContext => {
                if matcher.compartment_type == "Patient" {
                    context.environment.patient_context.clone()
                } else if matcher.compartment_type == "Encounter" {
                    context.environment.encounter_context.clone()
                } else {
                    None
                }
            }
            CompartmentIdSource::UserFhirResource => context
                .user
                .as_ref()
                .and_then(|u| u.fhir_user_id.clone())
                .filter(|_| {
                    context
                        .user
                        .as_ref()
                        .and_then(|u| u.fhir_user_type.as_ref())
                        .map(|t| t == &matcher.compartment_type)
                        .unwrap_or(false)
                }),
            CompartmentIdSource::Fixed { value } => Some(value.clone()),
            CompartmentIdSource::RequestParam { param } => {
                context.request.query_params.get(param).cloned()
            }
            CompartmentIdSource::TokenClaim { claim: _ } => {
                // Token claims are not directly accessible in PolicyContext
                // This would need to be enhanced if needed
                None
            }
        };

        let Some(compartment_id) = compartment_id else {
            return false;
        };

        // Check if resource is in compartment
        self.is_in_compartment(&matcher.compartment_type, &compartment_id, context)
    }

    /// Check if the resource in context is in the specified compartment.
    fn is_in_compartment(
        &self,
        compartment_type: &str,
        compartment_id: &str,
        context: &PolicyContext,
    ) -> bool {
        match compartment_type {
            "Patient" => self.is_in_patient_compartment(compartment_id, context),
            "Practitioner" => self.is_in_practitioner_compartment(compartment_id, context),
            _ => false,
        }
    }

    fn is_in_patient_compartment(&self, compartment_id: &str, context: &PolicyContext) -> bool {
        // Check if resource has matching subject
        if context
            .resource
            .as_ref()
            .is_some_and(|r| self.reference_matches(&r.subject, "Patient", compartment_id))
        {
            return true;
        }

        // Check if accessing Patient resource directly
        if context.request.resource_type == "Patient"
            && context.request.resource_id.as_deref() == Some(compartment_id)
        {
            return true;
        }

        // Check compartment from URL
        context.request.compartment_type.as_deref() == Some("Patient")
            && context.request.compartment_id.as_deref() == Some(compartment_id)
    }

    fn is_in_practitioner_compartment(
        &self,
        compartment_id: &str,
        context: &PolicyContext,
    ) -> bool {
        // Check if resource has matching author
        if context
            .resource
            .as_ref()
            .is_some_and(|r| self.reference_matches(&r.author, "Practitioner", compartment_id))
        {
            return true;
        }

        // Check if accessing Practitioner resource directly
        context.request.resource_type == "Practitioner"
            && context.request.resource_id.as_deref() == Some(compartment_id)
    }

    fn reference_matches(&self, reference: &Option<String>, resource_type: &str, id: &str) -> bool {
        reference.as_ref().is_some_and(|r| {
            let expected = format!("{}/{}", resource_type, id);
            r == &expected || r.ends_with(&format!("/{}", id))
        })
    }

    /// Check if a scope string contains a required scope.
    fn scope_matches(&self, scope_string: &str, required: &str) -> bool {
        scope_string.split_whitespace().any(|s| s == required)
    }

    /// Get a compiled regex from cache or compile and cache it.
    fn get_or_compile_regex(&self, pattern: &str) -> Option<Regex> {
        // Check cache first
        if let Some(re) = self
            .regex_cache
            .read()
            .ok()
            .and_then(|cache| cache.get(pattern).cloned())
        {
            return Some(re);
        }

        // Compile and cache
        let re = Regex::new(pattern).ok()?;
        if let Ok(mut cache) = self.regex_cache.write() {
            cache.insert(pattern.to_string(), re.clone());
        }

        Some(re)
    }
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
    use time::OffsetDateTime;

    // -------------------------------------------------------------------------
    // Test Helpers
    // -------------------------------------------------------------------------

    fn create_test_context() -> PolicyContext {
        PolicyContext {
            user: Some(UserIdentity {
                id: "user-123".to_string(),
                fhir_user: Some("Practitioner/456".to_string()),
                fhir_user_type: Some("Practitioner".to_string()),
                fhir_user_id: Some("456".to_string()),
                roles: vec!["doctor".to_string()],
                attributes: HashMap::new(),
            }),
            client: ClientIdentity {
                id: "app-client".to_string(),
                name: "Test App".to_string(),
                trusted: false,
                client_type: ClientType::Public,
            },
            scopes: ScopeSummary::from_scope_string("patient/Patient.r user/Observation.rs"),
            request: RequestContext {
                operation: FhirOperation::Read,
                operation_id: Some("fhir.read".to_string()),
                resource_type: "Patient".to_string(),
                resource_id: Some("789".to_string()),
                compartment_type: None,
                compartment_id: None,
                body: None,
                query_params: HashMap::new(),
                path: "/Patient/789".to_string(),
                method: "GET".to_string(),
            },
            resource: None,
            environment: EnvironmentContext {
                request_time: OffsetDateTime::now_utc(),
                source_ip: Some("192.168.1.100".parse().unwrap()),
                request_id: "req-123".to_string(),
                patient_context: Some("789".to_string()),
                encounter_context: None,
            },
        }
    }

    // -------------------------------------------------------------------------
    // Match Pattern Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_match_pattern_exact() {
        let pattern = MatchPattern::exact("test-client");
        assert!(pattern.matches("test-client"));
        assert!(!pattern.matches("test-client-2"));
        assert!(!pattern.matches("other"));
    }

    #[test]
    fn test_match_pattern_prefix() {
        let pattern = MatchPattern::prefix("app-");
        assert!(pattern.matches("app-client"));
        assert!(pattern.matches("app-123"));
        assert!(!pattern.matches("other-app"));
    }

    #[test]
    fn test_match_pattern_suffix() {
        let pattern = MatchPattern::suffix("-client");
        assert!(pattern.matches("app-client"));
        assert!(pattern.matches("test-client"));
        assert!(!pattern.matches("client-app"));
    }

    #[test]
    fn test_match_pattern_regex() {
        let pattern = MatchPattern::regex(r"^app-\d+$");
        assert!(pattern.matches("app-123"));
        assert!(pattern.matches("app-1"));
        assert!(!pattern.matches("app-abc"));
        assert!(!pattern.matches("other-123"));
    }

    #[test]
    fn test_match_pattern_wildcard() {
        let pattern = MatchPattern::wildcard();
        assert!(pattern.matches("anything"));
        assert!(pattern.matches(""));
        assert!(pattern.matches("12345"));
    }

    // -------------------------------------------------------------------------
    // Client Pattern Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_client_exact_match() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            clients: Some(vec![MatchPattern::exact("app-client")]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            clients: Some(vec![MatchPattern::exact("other-client")]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_client_prefix_match() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            clients: Some(vec![MatchPattern::prefix("app-")]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_client_multiple_patterns() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        // Any pattern matches = success
        let matchers = PolicyMatchers {
            clients: Some(vec![
                MatchPattern::exact("other-client"),
                MatchPattern::prefix("app-"),
            ]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Role Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_role_matching() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            roles: Some(vec!["doctor".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            roles: Some(vec!["admin".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_role_any_matches() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        // Any role matches = success
        let matchers = PolicyMatchers {
            roles: Some(vec!["admin".to_string(), "doctor".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_role_no_user() {
        let matcher = PatternMatcher::new();
        let mut context = create_test_context();
        context.user = None;

        let matchers = PolicyMatchers {
            roles: Some(vec!["doctor".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // User Type Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_user_type_matching() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            user_types: Some(vec!["Practitioner".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            user_types: Some(vec!["Patient".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Resource Type Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_resource_type_matching() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            resource_types: Some(vec!["Patient".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            resource_types: Some(vec!["Observation".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_resource_type_wildcard() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            resource_types: Some(vec!["*".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Operation Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_operation_matching() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            operations: Some(vec![FhirOperation::Read]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            operations: Some(vec![FhirOperation::Create, FhirOperation::Update]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // IP CIDR Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_ip_cidr_matching() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            source_ips: Some(vec!["192.168.1.0/24".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            source_ips: Some(vec!["10.0.0.0/8".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_ip_exact_match() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            source_ips: Some(vec!["192.168.1.100/32".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_ip_no_ip_in_context() {
        let matcher = PatternMatcher::new();
        let mut context = create_test_context();
        context.environment.source_ip = None;

        let matchers = PolicyMatchers {
            source_ips: Some(vec!["192.168.1.0/24".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Path Glob Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_path_glob_single_wildcard() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            paths: Some(vec!["/Patient/*".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            paths: Some(vec!["/Observation/*".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_path_glob_double_wildcard() {
        let matcher = PatternMatcher::new();
        let mut context = create_test_context();
        context.request.path = "/Patient/123/Observation".to_string();

        let matchers = PolicyMatchers {
            paths: Some(vec!["/Patient/**".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_path_exact() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            paths: Some(vec!["/Patient/789".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Required Scopes Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_required_scopes() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            required_scopes: Some(vec!["patient/Patient.r".to_string()]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));

        let matchers = PolicyMatchers {
            required_scopes: Some(vec!["system/Patient.r".to_string()]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Compartment Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_compartment_launch_context() {
        let matcher = PatternMatcher::new();
        let mut context = create_test_context();
        context.environment.patient_context = Some("789".to_string());

        let matchers = PolicyMatchers {
            compartments: Some(vec![CompartmentMatcher {
                compartment_type: "Patient".to_string(),
                compartment_id: CompartmentIdSource::LaunchContext,
            }]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_compartment_fixed_value() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            compartments: Some(vec![CompartmentMatcher {
                compartment_type: "Patient".to_string(),
                compartment_id: CompartmentIdSource::Fixed {
                    value: "789".to_string(),
                },
            }]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_compartment_with_resource_subject() {
        let matcher = PatternMatcher::new();
        let mut context = create_test_context();
        context.resource = Some(ResourceContext {
            resource: serde_json::json!({"resourceType": "Observation", "id": "obs-1"}),
            id: "obs-1".to_string(),
            resource_type: "Observation".to_string(),
            version_id: None,
            last_updated: None,
            subject: Some("Patient/789".to_string()),
            author: None,
        });
        context.environment.patient_context = Some("789".to_string());

        let matchers = PolicyMatchers {
            compartments: Some(vec![CompartmentMatcher {
                compartment_type: "Patient".to_string(),
                compartment_id: CompartmentIdSource::LaunchContext,
            }]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Combined Matchers Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_combined_matchers_all_match() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            roles: Some(vec!["doctor".to_string()]),
            resource_types: Some(vec!["Patient".to_string()]),
            operations: Some(vec![FhirOperation::Read]),
            ..Default::default()
        };
        assert!(matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_combined_matchers_one_fails() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers {
            roles: Some(vec!["admin".to_string()]), // Doesn't match
            resource_types: Some(vec!["Patient".to_string()]),
            operations: Some(vec![FhirOperation::Read]),
            ..Default::default()
        };
        assert!(!matcher.matches(&matchers, &context));
    }

    #[test]
    fn test_empty_matchers() {
        let matcher = PatternMatcher::new();
        let context = create_test_context();

        let matchers = PolicyMatchers::default();
        assert!(matcher.matches(&matchers, &context));
    }

    // -------------------------------------------------------------------------
    // Serialization Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_policy_matchers_serialization() {
        let matchers = PolicyMatchers {
            roles: Some(vec!["doctor".to_string()]),
            resource_types: Some(vec!["Patient".to_string()]),
            ..Default::default()
        };

        let json = serde_json::to_string(&matchers).unwrap();
        assert!(json.contains("roles"));
        assert!(json.contains("resourceTypes"));

        let parsed: PolicyMatchers = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.roles, matchers.roles);
    }

    #[test]
    fn test_match_pattern_serialization() {
        let pattern = MatchPattern::exact("test");
        let json = serde_json::to_string(&pattern).unwrap();
        assert!(json.contains(r#""type":"exact""#));

        let parsed: MatchPattern = serde_json::from_str(&json).unwrap();
        assert!(parsed.matches("test"));
    }

    // -------------------------------------------------------------------------
    // Regex Cache Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_regex_cache() {
        let matcher = PatternMatcher::new();

        // Call twice to test caching
        assert!(matcher.matches_glob("/Patient/*", "/Patient/123"));
        assert!(matcher.matches_glob("/Patient/*", "/Patient/456"));

        // Check cache has entries
        let cache = matcher.regex_cache.read().unwrap();
        assert!(!cache.is_empty());
    }
}
