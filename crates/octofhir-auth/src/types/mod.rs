//! Common types used across the authentication and authorization modules.
//!
//! This module contains shared type definitions that are used by multiple
//! submodules within the auth crate.
//!
//! ## Domain Types
//!
//! - [`Client`] - OAuth 2.0 client registration
//! - [`GrantType`] - Supported OAuth grant types

pub mod client;

pub use client::{Client, ClientValidationError, GrantType};
