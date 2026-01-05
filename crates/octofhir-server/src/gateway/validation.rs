//! Path validation for App operations.
//!
//! Validates that operation paths don't conflict with:
//! - System reserved paths (/fhir, /oauth, /admin, etc.)
//! - Other Apps' operation paths

use super::types::App;
use crate::app_platform::HttpMethod;

/// Reserved path prefixes that Apps cannot use.
const RESERVED_PATH_PREFIXES: &[&str] = &[
    "/fhir",
    "/oauth",
    "/admin",
    "/api",
    "/ui",
    "/healthz",
    "/readyz",
    "/metrics",
    "/$graphql",
];

/// Resource type paths that are reserved for FHIR resources.
/// Apps cannot create operations at these paths.
const RESERVED_RESOURCE_PATHS: &[&str] = &[
    "/User",
    "/Role",
    "/Client",
    "/AccessPolicy",
    "/IdentityProvider",
    "/CustomOperation",
    "/App",
];

/// Error type for path validation.
#[derive(Debug, Clone)]
pub enum PathValidationError {
    /// Operation path uses a reserved system prefix.
    ReservedPrefix {
        path: String,
        prefix: String,
        operation_id: String,
    },
    /// Operation path conflicts with FHIR resource endpoint.
    ReservedResourcePath {
        path: String,
        operation_id: String,
    },
    /// Operation path conflicts with another App's operation.
    ConflictWithOtherApp {
        path: String,
        method: String,
        operation_id: String,
        conflicting_app_id: String,
        conflicting_app_name: String,
    },
    /// Duplicate operation paths within the same App.
    DuplicateWithinApp {
        path: String,
        method: String,
        operation_id: String,
        duplicate_operation_id: String,
    },
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathValidationError::ReservedPrefix {
                path,
                prefix,
                operation_id,
            } => write!(
                f,
                "Operation '{}' path '{}' uses reserved system prefix '{}'",
                operation_id, path, prefix
            ),
            PathValidationError::ReservedResourcePath { path, operation_id } => write!(
                f,
                "Operation '{}' path '{}' conflicts with FHIR resource endpoint",
                operation_id, path
            ),
            PathValidationError::ConflictWithOtherApp {
                path,
                method,
                operation_id,
                conflicting_app_id,
                conflicting_app_name,
            } => write!(
                f,
                "Operation '{}' {} {} conflicts with App '{}' ({})",
                operation_id, method, path, conflicting_app_name, conflicting_app_id
            ),
            PathValidationError::DuplicateWithinApp {
                path,
                method,
                operation_id,
                duplicate_operation_id,
            } => write!(
                f,
                "Operation '{}' {} {} duplicates operation '{}'",
                operation_id, method, path, duplicate_operation_id
            ),
        }
    }
}

impl std::error::Error for PathValidationError {}

/// Validates an App's operations for path conflicts.
///
/// # Arguments
/// * `app` - The App being validated
/// * `other_apps` - Other existing Apps to check for conflicts
///
/// # Returns
/// * `Ok(())` if all operations are valid
/// * `Err(Vec<PathValidationError>)` with all validation errors
pub fn validate_app_operations(
    app: &App,
    other_apps: &[App],
) -> Result<(), Vec<PathValidationError>> {
    let mut errors = Vec::new();

    // Validate each operation
    for op in &app.operations {
        let path = op.path_string();

        // Check reserved prefixes
        for prefix in RESERVED_PATH_PREFIXES {
            if path.starts_with(prefix) {
                errors.push(PathValidationError::ReservedPrefix {
                    path: path.clone(),
                    prefix: (*prefix).to_string(),
                    operation_id: op.id.clone(),
                });
            }
        }

        // Check reserved resource paths
        for resource_path in RESERVED_RESOURCE_PATHS {
            if path.eq_ignore_ascii_case(resource_path)
                || path.starts_with(&format!("{}/", resource_path))
            {
                errors.push(PathValidationError::ReservedResourcePath {
                    path: path.clone(),
                    operation_id: op.id.clone(),
                });
            }
        }

        // Check conflicts with other apps
        for other_app in other_apps {
            // Skip self
            if other_app.id.as_deref() == app.id.as_deref() {
                continue;
            }

            // Skip inactive apps
            if !other_app.is_active() {
                continue;
            }

            for other_op in &other_app.operations {
                if paths_conflict(&path, &op.method, &other_op.path_string(), &other_op.method) {
                    errors.push(PathValidationError::ConflictWithOtherApp {
                        path: path.clone(),
                        method: op.method.to_string(),
                        operation_id: op.id.clone(),
                        conflicting_app_id: other_app.id.clone().unwrap_or_default(),
                        conflicting_app_name: other_app.name.clone(),
                    });
                }
            }
        }
    }

    // Check for duplicates within the same app
    for (i, op1) in app.operations.iter().enumerate() {
        for op2 in app.operations.iter().skip(i + 1) {
            let path1 = op1.path_string();
            let path2 = op2.path_string();

            if paths_conflict(&path1, &op1.method, &path2, &op2.method) {
                errors.push(PathValidationError::DuplicateWithinApp {
                    path: path1,
                    method: op1.method.to_string(),
                    operation_id: op1.id.clone(),
                    duplicate_operation_id: op2.id.clone(),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Checks if two paths conflict (same method and matching path pattern).
fn paths_conflict(path1: &str, method1: &HttpMethod, path2: &str, method2: &HttpMethod) -> bool {
    if method1 != method2 {
        return false;
    }
    paths_match(path1, path2)
}

/// Checks if two paths match, considering path parameters.
///
/// Path parameters (segments starting with ':') match any value.
/// Example: `/users/:id` matches `/users/123`
fn paths_match(path1: &str, path2: &str) -> bool {
    let segments1: Vec<&str> = path1.split('/').filter(|s| !s.is_empty()).collect();
    let segments2: Vec<&str> = path2.split('/').filter(|s| !s.is_empty()).collect();

    if segments1.len() != segments2.len() {
        return false;
    }

    segments1
        .iter()
        .zip(segments2.iter())
        .all(|(s1, s2)| s1.starts_with(':') || s2.starts_with(':') || s1 == s2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_platform::{AppEndpoint, AppStatus};
    use crate::gateway::types::InlineOperation;

    fn make_app(id: &str, name: &str, operations: Vec<InlineOperation>) -> App {
        App {
            id: Some(id.to_string()),
            resource_type: "App".to_string(),
            name: name.to_string(),
            description: None,
            api_version: Some(1),
            status: AppStatus::Active,
            secret: "test-secret".to_string(),
            endpoint: Some(AppEndpoint {
                url: "http://localhost:3000".to_string(),
                timeout: Some(30),
            }),
            operations,
            subscriptions: Vec::new(),
            resources: None,
            base_path: None,
            active: None,
        }
    }

    fn make_op(id: &str, method: HttpMethod, path: Vec<&str>) -> InlineOperation {
        InlineOperation {
            id: id.to_string(),
            method,
            path: path.into_iter().map(String::from).collect(),
            operation_type: "app".to_string(),
            public: false,
            policy: None,
            include_raw_body: None,
            websocket: None,
        }
    }

    #[test]
    fn test_reserved_prefix_fhir() {
        let app = make_app(
            "test",
            "Test",
            vec![make_op("op1", HttpMethod::Get, vec!["fhir", "Patient"])],
        );

        let result = validate_app_operations(&app, &[]);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            PathValidationError::ReservedPrefix { prefix, .. } if prefix == "/fhir"
        ));
    }

    #[test]
    fn test_reserved_prefix_oauth() {
        let app = make_app(
            "test",
            "Test",
            vec![make_op("op1", HttpMethod::Post, vec!["oauth", "token"])],
        );

        let result = validate_app_operations(&app, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reserved_resource_path() {
        let app = make_app(
            "test",
            "Test",
            vec![make_op("op1", HttpMethod::Get, vec!["User"])],
        );

        let result = validate_app_operations(&app, &[]);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(matches!(
            &errors[0],
            PathValidationError::ReservedResourcePath { .. }
        ));
    }

    #[test]
    fn test_valid_custom_path() {
        let app = make_app(
            "test",
            "Test",
            vec![make_op(
                "op1",
                HttpMethod::Post,
                vec!["myapp", "appointments", "book"],
            )],
        );

        let result = validate_app_operations(&app, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_conflict_with_other_app() {
        let app1 = make_app(
            "app1",
            "App One",
            vec![make_op(
                "op1",
                HttpMethod::Post,
                vec!["shared", "endpoint"],
            )],
        );

        let app2 = make_app(
            "app2",
            "App Two",
            vec![make_op(
                "op2",
                HttpMethod::Post,
                vec!["shared", "endpoint"],
            )],
        );

        let result = validate_app_operations(&app2, &[app1]);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(matches!(
            &errors[0],
            PathValidationError::ConflictWithOtherApp {
                conflicting_app_id,
                ..
            } if conflicting_app_id == "app1"
        ));
    }

    #[test]
    fn test_no_conflict_different_methods() {
        let app1 = make_app(
            "app1",
            "App One",
            vec![make_op(
                "op1",
                HttpMethod::Get,
                vec!["shared", "endpoint"],
            )],
        );

        let app2 = make_app(
            "app2",
            "App Two",
            vec![make_op(
                "op2",
                HttpMethod::Post,
                vec!["shared", "endpoint"],
            )],
        );

        let result = validate_app_operations(&app2, &[app1]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_conflict_with_path_params() {
        let app1 = make_app(
            "app1",
            "App One",
            vec![make_op("op1", HttpMethod::Get, vec!["users", ":id"])],
        );

        let app2 = make_app(
            "app2",
            "App Two",
            vec![make_op("op2", HttpMethod::Get, vec!["users", ":userId"])],
        );

        let result = validate_app_operations(&app2, &[app1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_within_app() {
        let app = make_app(
            "test",
            "Test",
            vec![
                make_op("op1", HttpMethod::Post, vec!["myapp", "action"]),
                make_op("op2", HttpMethod::Post, vec!["myapp", "action"]),
            ],
        );

        let result = validate_app_operations(&app, &[]);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(matches!(
            &errors[0],
            PathValidationError::DuplicateWithinApp { .. }
        ));
    }

    #[test]
    fn test_skip_inactive_apps() {
        let mut inactive_app = make_app(
            "inactive",
            "Inactive App",
            vec![make_op(
                "op1",
                HttpMethod::Post,
                vec!["shared", "endpoint"],
            )],
        );
        inactive_app.status = AppStatus::Inactive;

        let new_app = make_app(
            "new",
            "New App",
            vec![make_op(
                "op2",
                HttpMethod::Post,
                vec!["shared", "endpoint"],
            )],
        );

        // Should not conflict with inactive app
        let result = validate_app_operations(&new_app, &[inactive_app]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_paths_match() {
        assert!(paths_match("/users/123", "/users/:id"));
        assert!(paths_match("/users/:id", "/users/123"));
        assert!(paths_match("/users/:id", "/users/:userId"));
        assert!(!paths_match("/users", "/users/123"));
        assert!(!paths_match("/users/123/profile", "/users/123"));
        assert!(paths_match(
            "/apps/:appId/operations/:opId",
            "/apps/myapp/operations/op1"
        ));
    }
}
