//! Admin authentication extractor.
//!
//! This module provides an Axum extractor for validating admin access
//! to protected endpoints.
//!
//! # Example
//!
//! ```ignore
//! use axum::{Router, routing::get, Json};
//! use octofhir_auth::middleware::AdminAuth;
//!
//! async fn admin_handler(admin: AdminAuth) -> Json<String> {
//!     Json(format!("Hello admin: {}!", admin.username))
//! }
//!
//! let app = Router::new()
//!     .route("/admin", get(admin_handler))
//!     .with_state(auth_state);
//! ```

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;

use crate::error::AuthError;

use super::auth::{AuthState, BearerAuth};

// =============================================================================
// Admin Auth Extractor
// =============================================================================

/// Admin authentication context.
///
/// This extractor validates that the request has a valid Bearer token
/// and that the authenticated user has admin privileges.
#[derive(Debug, Clone)]
pub struct AdminAuth {
    /// User's unique identifier.
    pub user_id: String,

    /// Username for display/logging.
    pub username: String,

    /// User's assigned roles.
    pub roles: Vec<String>,
}

impl AdminAuth {
    /// Admin role names that grant admin access.
    const ADMIN_ROLES: &'static [&'static str] = &["admin", "superuser"];

    /// Returns `true` if the user has a specific role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

impl<S> FromRequestParts<S> for AdminAuth
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Extract and validate Bearer token
        let BearerAuth(auth) = BearerAuth::from_request_parts(parts, state).await?;

        // 2. Ensure a user is authenticated (not a client credentials token)
        let user = auth.user.ok_or_else(|| {
            tracing::debug!("Admin access denied: no user context (client credentials token)");
            AuthError::forbidden("Admin access requires user authentication")
        })?;

        // 3. Check for admin role
        let has_admin_role = user.roles.iter().any(|role| {
            Self::ADMIN_ROLES
                .iter()
                .any(|admin_role| role == *admin_role)
        });

        if !has_admin_role {
            tracing::debug!(
                user_id = %user.id,
                username = %user.username,
                roles = ?user.roles,
                "Admin access denied: missing admin role"
            );
            return Err(AuthError::forbidden("Admin access required"));
        }

        tracing::debug!(
            user_id = %user.id,
            username = %user.username,
            "Admin access granted"
        );

        Ok(Self {
            user_id: user.id,
            username: user.username,
            roles: user.roles,
        })
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_admin_auth_has_role() {
        let admin = AdminAuth {
            user_id: Uuid::new_v4().to_string(),
            username: "admin_user".to_string(),
            roles: vec!["admin".to_string(), "practitioner".to_string()],
        };

        assert!(admin.has_role("admin"));
        assert!(admin.has_role("practitioner"));
        assert!(!admin.has_role("patient"));
    }

    #[test]
    fn test_admin_roles_constant() {
        assert!(AdminAuth::ADMIN_ROLES.contains(&"admin"));
        assert!(AdminAuth::ADMIN_ROLES.contains(&"superuser"));
    }
}
