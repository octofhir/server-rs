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

pub mod client;
pub mod refresh_token;

pub use client::{Client, ClientValidationError, GrantType};
pub use refresh_token::RefreshToken;
