//! System routes for App integrations.
//!
//! Provides endpoints for Apps to interact with the server:
//! - `/api/system/logs` - Submit logs for centralized logging
//! - `/api/system/logs/batch` - Submit logs in batch format
//!
//! All routes require BasicAuth (HTTP Basic Auth) and only accept App entities.

mod logs;

pub use logs::*;

use axum::{routing::post, Router};

use crate::server::AppState;

/// Create system routes router.
///
/// All routes require BasicAuth (HTTP Basic Auth with app_id:secret).
/// Only Apps can access these routes (Clients will be rejected with 403 Forbidden).
pub fn system_routes() -> Router<AppState> {
    Router::new()
        .route("/logs", post(handle_app_logs))
        .route("/logs/batch", post(handle_app_logs_batch))
}
