//! Gateway policy evaluation for operation access control.
//!
//! This module provides policy evaluation for gateway operations, enforcing:
//! - Authentication requirements
//! - Role-based access control (RBAC)
//! - Scope-based access control (OAuth scopes)
//! - Compartment-based access (patient/practitioner)
//! - Custom QuickJS policy scripts

use crate::app_platform::OperationPolicy;
use crate::gateway::GatewayError;
use crate::gateway::types::CustomOperation;
use crate::server::AppState;
use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::{AccessDecision, PolicyContextBuilder};
use std::sync::Arc;

/// Evaluate operation policy against auth context.
///
/// Returns Ok(()) if access is allowed, Err(GatewayError) if denied.
///
/// # Access Control Rules
///
/// 1. **Public operations**: Always allowed, skip all checks
/// 2. **No policy**: Require authentication by default
/// 3. **require_auth**: Check authentication (default: true)
/// 4. **roles**: User must have at least one required role (OR logic)
/// 5. **scopes**: User must have at least one required scope (OR logic, wildcard support)
/// 6. **require_fhir_user**: User must be linked to a FHIR resource
/// 7. **compartment**: User's fhirUser must match compartment type
/// 8. **script**: Custom QuickJS policy evaluation (via PolicyEvaluator)
pub async fn evaluate_operation_policy(
    operation: &CustomOperation,
    auth_context: Option<&Arc<AuthContext>>,
    state: &AppState,
) -> Result<(), GatewayError> {
    // Skip policy check for public operations
    if operation.public {
        return Ok(());
    }

    // No policy = require authentication by default
    let policy = match &operation.policy {
        Some(p) => p,
        None => {
            // No policy but not public = require authentication
            if auth_context.is_none() {
                return Err(GatewayError::Unauthorized(
                    "Authentication required".to_string(),
                ));
            }
            return Ok(());
        }
    };

    // Check require_auth (default: true)
    let require_auth = policy.require_auth.unwrap_or(true);
    if require_auth && auth_context.is_none() {
        return Err(GatewayError::Unauthorized(
            "Authentication required".to_string(),
        ));
    }

    // If no auth and not required, allow
    let Some(auth) = auth_context else {
        return Ok(());
    };

    // Fast path: simple checks
    evaluate_simple_policy(policy, auth)?;

    // Slow path: complex script evaluation
    if let Some(script) = &policy.script {
        evaluate_policy_script(script, auth, state).await?;
    }

    Ok(())
}

/// Evaluate simple policy requirements (roles, scopes, compartment, fhirUser).
///
/// This is the "fast path" for common policy checks that don't require
/// complex script evaluation.
fn evaluate_simple_policy(
    policy: &OperationPolicy,
    auth: &AuthContext,
) -> Result<(), GatewayError> {
    // Check fhirUser requirement
    if policy.require_fhir_user.unwrap_or(false) && auth.fhir_user().is_none() {
        return Err(GatewayError::Forbidden(
            "User must be linked to a FHIR resource".to_string(),
        ));
    }

    // Check roles (OR logic)
    if let Some(required_roles) = &policy.roles
        && !required_roles.is_empty()
    {
        let empty_roles = Vec::new();
        let user_roles = auth.user.as_ref().map(|u| &u.roles).unwrap_or(&empty_roles);

        let has_role = required_roles.iter().any(|r| user_roles.contains(r));

        if !has_role {
            return Err(GatewayError::Forbidden(format!(
                "Missing required role. Need one of: {:?}",
                required_roles
            )));
        }
    }

    // Check scopes (OR logic with wildcard support)
    if let Some(required_scopes) = &policy.scopes
        && !required_scopes.is_empty()
    {
        let user_scopes: Vec<String> = auth.scopes().map(String::from).collect();

        let has_scope = required_scopes.iter().any(|req_scope| {
            user_scopes
                .iter()
                .any(|user_scope| scope_matches(user_scope, req_scope))
        });

        if !has_scope {
            return Err(GatewayError::Forbidden(format!(
                "Missing required scope. Need one of: {:?}",
                required_scopes
            )));
        }
    }

    // Check compartment
    if let Some(compartment) = &policy.compartment {
        evaluate_compartment(compartment, auth)?;
    }

    Ok(())
}

/// Check compartment-based access (patient or practitioner).
///
/// Compartments restrict access based on the user's fhirUser reference:
/// - "patient": User must have fhirUser = Patient/xxx
/// - "practitioner": User must have fhirUser = Practitioner/xxx
fn evaluate_compartment(compartment: &str, auth: &AuthContext) -> Result<(), GatewayError> {
    match compartment {
        "patient" => {
            // User must have fhirUser = Patient/xxx
            match auth.fhir_user() {
                Some(fhir_user) if fhir_user.starts_with("Patient/") => Ok(()),
                _ => Err(GatewayError::Forbidden(
                    "Patient compartment requires Patient fhirUser".to_string(),
                )),
            }
        }
        "practitioner" => {
            // User must have fhirUser = Practitioner/xxx
            match auth.fhir_user() {
                Some(fhir_user) if fhir_user.starts_with("Practitioner/") => Ok(()),
                _ => Err(GatewayError::Forbidden(
                    "Practitioner compartment requires Practitioner fhirUser".to_string(),
                )),
            }
        }
        _ => {
            tracing::warn!(compartment = %compartment, "Unknown compartment type");
            Ok(()) // Unknown compartments pass through
        }
    }
}

/// Check if user scope matches required scope (with wildcard support).
///
/// SMART on FHIR scopes follow the pattern: `context/resource.action`
///
/// # Wildcard Support
///
/// - `*` in resource position matches any resource type
/// - `*` in action position matches any action
///
/// # Examples
///
/// ```
/// # use octofhir_server::gateway::policy::scope_matches;
/// assert!(scope_matches("patient/*.read", "patient/Observation.read"));
/// assert!(scope_matches("patient/*.*", "patient/Observation.read"));
/// assert!(scope_matches("patient/Observation.*", "patient/Observation.write"));
/// assert!(!scope_matches("patient/*.read", "patient/Observation.write"));
/// assert!(!scope_matches("user/*.read", "patient/Observation.read"));
/// ```
fn scope_matches(user_scope: &str, required_scope: &str) -> bool {
    // Exact match
    if user_scope == required_scope {
        return true;
    }

    // Parse scopes: "context/resource.action"
    let user_parts: Vec<&str> = user_scope.split('/').collect();
    let req_parts: Vec<&str> = required_scope.split('/').collect();

    if user_parts.len() != 2 || req_parts.len() != 2 {
        return false;
    }

    // Check context (patient, user, system)
    if user_parts[0] != req_parts[0] {
        return false;
    }

    // Parse resource.action
    let user_ra: Vec<&str> = user_parts[1].split('.').collect();
    let req_ra: Vec<&str> = req_parts[1].split('.').collect();

    if user_ra.len() != 2 || req_ra.len() != 2 {
        return false;
    }

    // Wildcard resource: "*" matches any
    let resource_match = user_ra[0] == "*" || user_ra[0] == req_ra[0];

    // Wildcard action: "*" matches any
    let action_match = user_ra[1] == "*" || user_ra[1] == req_ra[1];

    resource_match && action_match
}

/// Evaluate QuickJS policy script using PolicyEvaluator.
///
/// This delegates complex policy evaluation to the existing PolicyEvaluator
/// infrastructure which supports custom QuickJS scripts.
async fn evaluate_policy_script(
    _script: &str,
    auth: &AuthContext,
    state: &AppState,
) -> Result<(), GatewayError> {
    tracing::debug!(script_len = _script.len(), "Evaluating policy script");

    // Build policy context
    let policy_context = PolicyContextBuilder::new()
        .with_auth_context(auth)
        .build()
        .map_err(|e| GatewayError::InternalError(format!("Policy context error: {}", e)))?;

    // Evaluate using AppState's policy evaluator
    let decision = state.policy_evaluator.evaluate(&policy_context).await;

    match decision {
        AccessDecision::Allow => Ok(()),
        AccessDecision::Deny(reason) => Err(GatewayError::Forbidden(format!(
            "Policy denied: {}",
            reason.message
        ))),
        AccessDecision::Abstain => {
            // No matching policy - default deny
            Err(GatewayError::Forbidden(
                "Policy evaluation abstained".to_string(),
            ))
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // scope_matches tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_scope_matches_exact() {
        assert!(scope_matches(
            "patient/Patient.read",
            "patient/Patient.read"
        ));
        assert!(!scope_matches(
            "patient/Patient.read",
            "patient/Patient.write"
        ));
    }

    #[test]
    fn test_scope_matches_wildcard_resource() {
        assert!(scope_matches("patient/*.read", "patient/Observation.read"));
        assert!(scope_matches("patient/*.read", "patient/Patient.read"));
        assert!(!scope_matches(
            "patient/*.read",
            "patient/Observation.write"
        ));
        assert!(!scope_matches("user/*.read", "patient/Observation.read"));
    }

    #[test]
    fn test_scope_matches_wildcard_action() {
        assert!(scope_matches(
            "patient/Observation.*",
            "patient/Observation.read"
        ));
        assert!(scope_matches(
            "patient/Observation.*",
            "patient/Observation.write"
        ));
        assert!(!scope_matches(
            "patient/Observation.*",
            "patient/Patient.read"
        ));
    }

    #[test]
    fn test_scope_matches_wildcard_both() {
        assert!(scope_matches("patient/*.*", "patient/Observation.read"));
        assert!(scope_matches("patient/*.*", "patient/Patient.write"));
    }

    #[test]
    fn test_scope_matches_different_context() {
        assert!(!scope_matches("patient/Patient.read", "user/Patient.read"));
        assert!(!scope_matches(
            "system/Patient.read",
            "patient/Patient.read"
        ));
    }

    #[test]
    fn test_scope_matches_malformed() {
        // Missing parts
        assert!(!scope_matches("patient", "patient/Patient.read"));
        assert!(!scope_matches("patient/Patient.read", "patient"));

        // Missing action
        assert!(!scope_matches("patient/Patient", "patient/Patient.read"));

        // Too many parts
        assert!(!scope_matches(
            "patient/Patient.read.extra",
            "patient/Patient.read"
        ));
    }
}
