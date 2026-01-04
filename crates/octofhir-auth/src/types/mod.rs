//! Common types used across the authentication and authorization modules.
//!
//! This module contains shared type definitions that are used by multiple
//! submodules within the auth crate.
//!
//! ## Domain Types
//!
//! - [`Client`] - OAuth 2.0 client registration
//! - [`GrantType`] - Supported OAuth grant types
//! - [`RefreshToken`] - Refresh token for offline access
//! - [`AppRecord`] - App record for authentication

pub mod app;
pub mod client;
pub mod refresh_token;

pub use app::AppRecord;
pub use client::{Client, ClientValidationError, GrantType};
pub use refresh_token::RefreshToken;

/// Entity type for Basic Auth (Client or App).
#[derive(Debug, Clone)]
pub enum BasicAuthEntity {
    /// OAuth Client
    Client(Client),
    /// Application
    App(AppRecord),
}

impl BasicAuthEntity {
    /// Get the entity ID (client_id or app_id).
    pub fn id(&self) -> &str {
        match self {
            BasicAuthEntity::Client(client) => &client.client_id,
            BasicAuthEntity::App(app) => &app.id,
        }
    }

    /// Check if entity is a client.
    pub fn is_client(&self) -> bool {
        matches!(self, BasicAuthEntity::Client(_))
    }

    /// Check if entity is an app.
    pub fn is_app(&self) -> bool {
        matches!(self, BasicAuthEntity::App(_))
    }
}
