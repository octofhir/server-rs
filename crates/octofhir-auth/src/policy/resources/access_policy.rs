//! AccessPolicy resource type for access control.
//!
//! This module defines the AccessPolicy FHIR-like resource for configuring
//! access control policies with pattern matching and scriptable engines.
//!
//! # Example
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
//!     matcher: Some(MatcherElement {
//!         roles: Some(vec!["admin".to_string()]),
//!         operations: Some(vec!["read".to_string()]),
//!         ..Default::default()
//!     }),
//!     ..Default::default()
//! };
//!
//! policy.validate()?;
//! let internal = policy.to_internal_policy()?;
//! ```

use serde::{Deserialize, Serialize};

use crate::policy::matcher::{MatchPattern, PolicyMatchers};
use crate::smart::scopes::FhirOperation;

// =============================================================================
// AccessPolicy Resource
// =============================================================================

/// AccessPolicy FHIR-like resource for access control.
///
/// This resource defines access control policies that determine whether
/// a request should be allowed or denied based on matching criteria and
/// policy engine evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPolicy {
    /// Resource type - always "AccessPolicy".
    #[serde(default = "default_resource_type")]
    pub resource_type: String,

    /// Unique identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Resource metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResourceMeta>,

    /// Human-readable policy name.
    pub name: String,

    /// Detailed description of the policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether the policy is active.
    #[serde(default = "default_active")]
    pub active: bool,

    /// Evaluation order (lower = evaluated first, range: 0-1000).
    #[serde(default = "default_priority")]
    pub priority: i32,

    /// Matcher element - determines when this policy applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<MatcherElement>,

    /// Engine element - how the policy is evaluated.
    pub engine: EngineElement,

    /// Custom message when access is denied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_message: Option<String>,
}

fn default_resource_type() -> String {
    "AccessPolicy".to_string()
}

fn default_active() -> bool {
    true
}

fn default_priority() -> i32 {
    100
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            resource_type: default_resource_type(),
            id: None,
            meta: None,
            name: String::new(),
            description: None,
            active: true,
            priority: 100,
            matcher: None,
            engine: EngineElement {
                engine_type: PolicyEngineType::Deny,
                script: None,
            },
            deny_message: None,
        }
    }
}

// =============================================================================
// Resource Metadata
// =============================================================================

/// Resource metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMeta {
    /// Version ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,

    /// Last updated timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

// =============================================================================
// Matcher Element
// =============================================================================

/// Matcher element - determines when a policy applies.
///
/// All specified fields must match for the policy to apply (AND logic).
/// Fields set to `None` are not evaluated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatcherElement {
    /// Client ID patterns (supports wildcards with `*`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<String>>,

    /// Required user roles (any role matches).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,

    /// User FHIR resource types (e.g., "Practitioner", "Patient").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_types: Option<Vec<String>>,

    /// Target FHIR resource types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_types: Option<Vec<String>>,

    /// FHIR operations (e.g., "read", "create", "search").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<String>>,

    /// Request path patterns (glob syntax).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,

    /// Source IP addresses in CIDR notation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ips: Option<Vec<String>>,
}

// =============================================================================
// Engine Element
// =============================================================================

/// Engine element - how the policy is evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineElement {
    /// Engine type.
    #[serde(rename = "type")]
    pub engine_type: PolicyEngineType,

    /// Script content (required for QuickJS engine).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

/// Policy engine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyEngineType {
    /// Always allow access.
    Allow,
    /// Always deny access.
    Deny,
    /// Evaluate using QuickJS script.
    #[serde(rename = "quickjs")]
    QuickJs,
}

// =============================================================================
// Validation
// =============================================================================

impl AccessPolicy {
    /// Validate the policy configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy configuration is invalid.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Name is required
        if self.name.is_empty() {
            return Err(ValidationError::MissingField("name"));
        }

        // Script is required for QuickJS engine
        match self.engine.engine_type {
            PolicyEngineType::QuickJs => {
                if self
                    .engine
                    .script
                    .as_ref()
                    .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(ValidationError::MissingField("engine.script"));
                }
            }
            PolicyEngineType::Allow | PolicyEngineType::Deny => {}
        }

        // Validate matcher if present
        if let Some(ref matcher) = self.matcher {
            self.validate_matcher(matcher)?;
        }

        // Validate priority range
        if !(0..=1000).contains(&self.priority) {
            return Err(ValidationError::InvalidPriority(self.priority));
        }

        Ok(())
    }

    fn validate_matcher(&self, matcher: &MatcherElement) -> Result<(), ValidationError> {
        // Validate operations
        if let Some(ref ops) = matcher.operations {
            for op in ops {
                Self::validate_operation(op)?;
            }
        }

        // Validate user types
        if let Some(ref types) = matcher.user_types {
            const VALID_USER_TYPES: &[&str] =
                &["Practitioner", "Patient", "RelatedPerson", "Person", "*"];
            for t in types {
                if !VALID_USER_TYPES.contains(&t.as_str()) {
                    return Err(ValidationError::InvalidUserType(t.clone()));
                }
            }
        }

        // Validate IP CIDRs
        if let Some(ref ips) = matcher.source_ips {
            for ip in ips {
                ip.parse::<ipnetwork::IpNetwork>()
                    .map_err(|_| ValidationError::InvalidCidr(ip.clone()))?;
            }
        }

        Ok(())
    }

    fn validate_operation(op: &str) -> Result<(), ValidationError> {
        const VALID_OPS: &[&str] = &[
            "read",
            "vread",
            "update",
            "patch",
            "delete",
            "history",
            "history-instance",
            "history-type",
            "history-system",
            "create",
            "search",
            "search-type",
            "search-system",
            "capabilities",
            "batch",
            "transaction",
            "operation",
            "*",
        ];

        if !VALID_OPS.contains(&op) {
            return Err(ValidationError::InvalidOperation(op.to_string()));
        }

        Ok(())
    }
}

/// Errors that can occur during policy validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A required field is missing.
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// Priority is out of range.
    #[error("Priority must be between 0 and 1000, got {0}")]
    InvalidPriority(i32),

    /// Invalid operation name.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Invalid user type.
    #[error("Invalid user type: {0} (must be Practitioner, Patient, RelatedPerson, Person, or *)")]
    InvalidUserType(String),

    /// Invalid CIDR notation.
    #[error("Invalid CIDR notation: {0}")]
    InvalidCidr(String),
}

// =============================================================================
// Internal Policy Representation
// =============================================================================

/// Internal representation of a policy for evaluation.
#[derive(Debug, Clone)]
pub struct InternalPolicy {
    /// Policy ID.
    pub id: String,

    /// Policy name.
    pub name: String,

    /// Evaluation priority.
    pub priority: i32,

    /// Matchers for determining when this policy applies.
    pub matchers: PolicyMatchers,

    /// Policy engine for evaluation.
    pub engine: PolicyEngine,

    /// Custom deny message.
    pub deny_message: Option<String>,
}

/// Policy engine for evaluation.
#[derive(Debug, Clone)]
pub enum PolicyEngine {
    /// Always allow access.
    Allow,
    /// Always deny access.
    Deny,
    /// Evaluate using QuickJS script.
    QuickJs {
        /// The JavaScript code to execute.
        script: String,
    },
}

// =============================================================================
// Conversion to Internal Policy
// =============================================================================

impl AccessPolicy {
    /// Convert to internal policy representation.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion fails.
    pub fn to_internal_policy(&self) -> Result<InternalPolicy, ConversionError> {
        let matchers = self.convert_matchers();
        let engine = self.convert_engine()?;

        Ok(InternalPolicy {
            id: self.id.clone().unwrap_or_default(),
            name: self.name.clone(),
            priority: self.priority,
            matchers,
            engine,
            deny_message: self.deny_message.clone(),
        })
    }

    fn convert_matchers(&self) -> PolicyMatchers {
        let Some(ref m) = self.matcher else {
            return PolicyMatchers::default();
        };

        PolicyMatchers {
            clients: m.clients.as_ref().map(|clients| {
                clients
                    .iter()
                    .map(|c| {
                        if c.contains('*') {
                            // Convert wildcard to regex
                            MatchPattern::Regex {
                                pattern: format!("^{}$", c.replace('*', ".*")),
                            }
                        } else {
                            MatchPattern::Exact { value: c.clone() }
                        }
                    })
                    .collect()
            }),
            roles: m.roles.clone(),
            user_types: m.user_types.clone(),
            resource_types: m.resource_types.clone(),
            operations: m
                .operations
                .as_ref()
                .map(|ops| ops.iter().flat_map(|o| Self::parse_operation(o)).collect()),
            paths: m.paths.clone(),
            source_ips: m.source_ips.clone(),
            compartments: None,
            required_scopes: None,
        }
    }

    fn convert_engine(&self) -> Result<PolicyEngine, ConversionError> {
        match self.engine.engine_type {
            PolicyEngineType::Allow => Ok(PolicyEngine::Allow),
            PolicyEngineType::Deny => Ok(PolicyEngine::Deny),
            PolicyEngineType::QuickJs => {
                let script = self
                    .engine
                    .script
                    .clone()
                    .ok_or(ConversionError::MissingScript)?;
                Ok(PolicyEngine::QuickJs { script })
            }
        }
    }

    fn parse_operation(op: &str) -> Vec<FhirOperation> {
        match op {
            "read" => vec![FhirOperation::Read],
            "vread" => vec![FhirOperation::VRead],
            "update" => vec![FhirOperation::Update],
            "patch" => vec![FhirOperation::Patch],
            "delete" => vec![FhirOperation::Delete],
            "create" => vec![FhirOperation::Create],
            "search" => vec![FhirOperation::Search],
            "search-type" => vec![FhirOperation::SearchType],
            "search-system" => vec![FhirOperation::SearchSystem],
            "capabilities" => vec![FhirOperation::Capabilities],
            "batch" => vec![FhirOperation::Batch],
            "transaction" => vec![FhirOperation::Transaction],
            "operation" => vec![FhirOperation::Operation],
            "history" => vec![
                FhirOperation::HistoryInstance,
                FhirOperation::HistoryType,
                FhirOperation::HistorySystem,
            ],
            "history-instance" => vec![FhirOperation::HistoryInstance],
            "history-type" => vec![FhirOperation::HistoryType],
            "history-system" => vec![FhirOperation::HistorySystem],
            "*" => vec![
                FhirOperation::Read,
                FhirOperation::VRead,
                FhirOperation::Update,
                FhirOperation::Patch,
                FhirOperation::Delete,
                FhirOperation::Create,
                FhirOperation::Search,
                FhirOperation::SearchType,
                FhirOperation::SearchSystem,
                FhirOperation::Capabilities,
                FhirOperation::Batch,
                FhirOperation::Transaction,
                FhirOperation::Operation,
                FhirOperation::HistoryInstance,
                FhirOperation::HistoryType,
                FhirOperation::HistorySystem,
            ],
            _ => vec![],
        }
    }
}

/// Errors that can occur during conversion to internal policy.
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    /// Script is missing for a scripted engine.
    #[error("Missing script for scripted engine")]
    MissingScript,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Validation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_valid_allow_policy() {
        let policy = AccessPolicy {
            name: "Allow all reads".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                operations: Some(vec!["read".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_valid_deny_policy() {
        let policy = AccessPolicy {
            name: "Deny all writes".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Deny,
                script: None,
            },
            matcher: Some(MatcherElement {
                operations: Some(vec!["create".to_string(), "update".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_valid_quickjs_policy() {
        let policy = AccessPolicy {
            name: "QuickJS policy".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::QuickJs,
                script: Some(r#"user.roles.includes("admin")"#.to_string()),
            },
            ..Default::default()
        };

        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_missing_name() {
        let policy = AccessPolicy {
            name: String::new(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(result, Err(ValidationError::MissingField("name"))));
    }

    #[test]
    fn test_invalid_operation() {
        let policy = AccessPolicy {
            name: "Bad operations".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                operations: Some(vec!["invalid_op".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(result, Err(ValidationError::InvalidOperation(_))));
    }

    #[test]
    fn test_invalid_user_type() {
        let policy = AccessPolicy {
            name: "Bad user type".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                user_types: Some(vec!["InvalidType".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(result, Err(ValidationError::InvalidUserType(_))));
    }

    #[test]
    fn test_invalid_cidr() {
        let policy = AccessPolicy {
            name: "Bad CIDR".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                source_ips: Some(vec!["not-a-cidr".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_valid_cidr() {
        let policy = AccessPolicy {
            name: "Valid CIDR".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                source_ips: Some(vec!["192.168.1.0/24".to_string(), "10.0.0.0/8".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_priority_out_of_range() {
        let policy = AccessPolicy {
            name: "Bad priority".to_string(),
            priority: 1001, // Out of range
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPriority(1001))
        ));
    }

    #[test]
    fn test_priority_negative() {
        let policy = AccessPolicy {
            name: "Negative priority".to_string(),
            priority: -1,
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        };

        let result = policy.validate();
        assert!(matches!(result, Err(ValidationError::InvalidPriority(-1))));
    }

    // -------------------------------------------------------------------------
    // Conversion Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_convert_to_internal_allow() {
        let policy = AccessPolicy {
            id: Some("policy-1".to_string()),
            name: "Test policy".to_string(),
            priority: 50,
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                roles: Some(vec!["admin".to_string()]),
                resource_types: Some(vec!["Patient".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let internal = policy.to_internal_policy().unwrap();

        assert_eq!(internal.id, "policy-1");
        assert_eq!(internal.name, "Test policy");
        assert_eq!(internal.priority, 50);
        assert!(matches!(internal.engine, PolicyEngine::Allow));
        assert!(internal.matchers.roles.is_some());
        assert!(internal.matchers.resource_types.is_some());
    }

    #[test]
    fn test_convert_client_wildcard() {
        let policy = AccessPolicy {
            name: "Wildcard clients".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                clients: Some(vec!["app-*".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let internal = policy.to_internal_policy().unwrap();
        let clients = internal.matchers.clients.unwrap();

        match &clients[0] {
            MatchPattern::Regex { pattern } => {
                assert_eq!(pattern, "^app-.*$");
            }
            _ => panic!("Expected Regex pattern"),
        }
    }

    #[test]
    fn test_convert_client_exact() {
        let policy = AccessPolicy {
            name: "Exact client".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                clients: Some(vec!["my-app".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let internal = policy.to_internal_policy().unwrap();
        let clients = internal.matchers.clients.unwrap();

        match &clients[0] {
            MatchPattern::Exact { value } => {
                assert_eq!(value, "my-app");
            }
            _ => panic!("Expected Exact pattern"),
        }
    }

    #[test]
    fn test_convert_operations() {
        let policy = AccessPolicy {
            name: "Operations".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                operations: Some(vec![
                    "read".to_string(),
                    "create".to_string(),
                    "history".to_string(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let internal = policy.to_internal_policy().unwrap();
        let ops = internal.matchers.operations.unwrap();

        // "history" expands to 3 operations
        assert!(ops.contains(&FhirOperation::Read));
        assert!(ops.contains(&FhirOperation::Create));
        assert!(ops.contains(&FhirOperation::HistoryInstance));
        assert!(ops.contains(&FhirOperation::HistoryType));
        assert!(ops.contains(&FhirOperation::HistorySystem));
    }

    #[test]
    fn test_convert_no_matcher() {
        let policy = AccessPolicy {
            name: "No matcher".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: None,
            ..Default::default()
        };

        let internal = policy.to_internal_policy().unwrap();

        assert!(internal.matchers.clients.is_none());
        assert!(internal.matchers.roles.is_none());
        assert!(internal.matchers.operations.is_none());
    }

    // -------------------------------------------------------------------------
    // Serialization Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_serialization_roundtrip() {
        let policy = AccessPolicy {
            id: Some("test".to_string()),
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            active: true,
            priority: 100,
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                roles: Some(vec!["admin".to_string()]),
                operations: Some(vec!["read".to_string()]),
                ..Default::default()
            }),
            deny_message: Some("Access denied".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: AccessPolicy = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, policy.id);
        assert_eq!(deserialized.name, policy.name);
        assert_eq!(deserialized.description, policy.description);
        assert_eq!(deserialized.active, policy.active);
        assert_eq!(deserialized.priority, policy.priority);
        assert_eq!(deserialized.deny_message, policy.deny_message);
    }

    #[test]
    fn test_serialization_format() {
        let policy = AccessPolicy {
            id: Some("test".to_string()),
            name: "Test Policy".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&policy).unwrap();

        assert!(json.contains(r#""resourceType":"AccessPolicy""#));
        assert!(json.contains(r#""name":"Test Policy""#));
        assert!(json.contains(r#""type":"allow""#));
    }

    #[test]
    fn test_serialization_quickjs() {
        let policy = AccessPolicy {
            name: "QuickJS".to_string(),
            engine: EngineElement {
                engine_type: PolicyEngineType::QuickJs,
                script: Some("true".to_string()),
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains(r#""type":"quickjs""#));
    }

    #[test]
    fn test_deserialization_minimal() {
        let json = r#"{
            "resourceType": "AccessPolicy",
            "name": "Minimal Policy",
            "engine": {
                "type": "deny"
            }
        }"#;

        let policy: AccessPolicy = serde_json::from_str(json).unwrap();

        assert_eq!(policy.name, "Minimal Policy");
        assert_eq!(policy.engine.engine_type, PolicyEngineType::Deny);
        assert!(policy.active); // Default
        assert_eq!(policy.priority, 100); // Default
    }

    // -------------------------------------------------------------------------
    // Default Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_default_policy() {
        let policy = AccessPolicy::default();

        assert_eq!(policy.resource_type, "AccessPolicy");
        assert!(policy.id.is_none());
        assert!(policy.name.is_empty());
        assert!(policy.active);
        assert_eq!(policy.priority, 100);
        assert!(policy.matcher.is_none());
        assert_eq!(policy.engine.engine_type, PolicyEngineType::Deny);
    }

    #[test]
    fn test_default_matcher_element() {
        let matcher = MatcherElement::default();

        assert!(matcher.clients.is_none());
        assert!(matcher.roles.is_none());
        assert!(matcher.user_types.is_none());
        assert!(matcher.resource_types.is_none());
        assert!(matcher.operations.is_none());
        assert!(matcher.paths.is_none());
        assert!(matcher.source_ips.is_none());
    }
}
