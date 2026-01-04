//! Axum extractors for authentication.
//!
//! This module provides Axum extractors that handle authentication
//! and extract authentication context from HTTP requests.
//!
//! ## Extractors
//!
//! - [`BasicAuth`] - Universal HTTP Basic Auth for Client and App authentication
//! - [`BasicAuthState`] - State container for basic authentication
//!
//! ## Authentication Logging
//!
//! All authentication attempts are logged with structured fields for audit trail:
//!
//! ### Successful Authentication
//!
//! When an entity successfully authenticates via [`BasicAuth`], the following is logged:
//!
//! ```text
//! INFO entity_id="app-123" entity_type="app" endpoint="/api/system/logs" method="POST" "Entity authenticated via Basic Auth"
//! ```
//!
//! ### Usage Example
//!
//! ```ignore
//! use octofhir_auth::extractors::BasicAuth;
//! use axum::{Json, http::StatusCode};
//!
//! async fn handler(auth: BasicAuth) -> Result<StatusCode, StatusCode> {
//!     // Check entity type
//!     if !auth.is_app() {
//!         return Err(StatusCode::FORBIDDEN);
//!     }
//!
//!     // Extract entity info
//!     let entity_id = &auth.entity_id;
//!
//!     tracing::info!(
//!         entity_id = %entity_id,
//!         "Processing request"
//!     );
//!
//!     Ok(StatusCode::OK)
//! }
//! ```
//!
//! ### Structured Fields
//!
//! All authentication logs include these fields:
//!
//! - `entity_id` - Client ID or App ID (always present)
//! - `entity_type` - "client" or "app" (always present)
//! - `endpoint` - API endpoint path
//! - `method` - HTTP method

mod basic_auth;

pub use basic_auth::{BasicAuth, BasicAuthError, BasicAuthState};
