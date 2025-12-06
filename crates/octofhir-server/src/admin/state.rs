//! Admin API state.
//!
//! This module provides the application state required for admin endpoints.

use std::sync::Arc;

use axum::extract::FromRef;
use octofhir_auth_postgres::PgPool;

use octofhir_auth::federation::IdpAuthService;
use octofhir_auth::middleware::AuthState;

use super::policy::PolicyState;

// =============================================================================
// Admin State
// =============================================================================

/// Application state for admin endpoints.
///
/// This struct should be included in your application state and made
/// available to admin handlers via `FromRef`.
#[derive(Clone)]
pub struct AdminState {
    /// PostgreSQL connection pool for storage operations.
    pub pool: Arc<PgPool>,

    /// IdP authentication service for reloading providers after changes.
    pub idp_auth_service: Option<Arc<IdpAuthService>>,
}

impl AdminState {
    /// Creates a new admin state.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self {
            pool,
            idp_auth_service: None,
        }
    }

    /// Sets the IdP authentication service.
    #[must_use]
    pub fn with_idp_auth_service(mut self, service: Arc<IdpAuthService>) -> Self {
        self.idp_auth_service = Some(service);
        self
    }
}

// =============================================================================
// Combined State
// =============================================================================

/// Combined state for admin handlers that need both auth and admin state.
///
/// This allows handlers to use `State(state): State<CombinedAdminState>` and
/// have access to both `AuthState` (for `AdminAuth` extractor) and `AdminState`.
#[derive(Clone)]
pub struct CombinedAdminState {
    /// Authentication state for token validation.
    pub auth: AuthState,

    /// Admin state for storage operations.
    pub admin: AdminState,

    /// Policy state for policy management (optional until initialized).
    pub policy: Option<PolicyState>,
}

impl CombinedAdminState {
    /// Creates a new combined admin state.
    #[must_use]
    pub fn new(auth: AuthState, admin: AdminState) -> Self {
        Self {
            auth,
            admin,
            policy: None,
        }
    }

    /// Sets the policy state.
    #[must_use]
    pub fn with_policy_state(mut self, policy: PolicyState) -> Self {
        self.policy = Some(policy);
        self
    }
}

impl FromRef<CombinedAdminState> for AuthState {
    fn from_ref(state: &CombinedAdminState) -> Self {
        state.auth.clone()
    }
}

impl FromRef<CombinedAdminState> for AdminState {
    fn from_ref(state: &CombinedAdminState) -> Self {
        state.admin.clone()
    }
}

impl FromRef<CombinedAdminState> for PolicyState {
    fn from_ref(state: &CombinedAdminState) -> Self {
        state
            .policy
            .clone()
            .expect("PolicyState not initialized in CombinedAdminState")
    }
}
