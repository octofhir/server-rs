//! Storage traits for authentication and authorization data.
//!
//! This module defines storage interfaces for:
//!
//! - OAuth client registrations
//! - Authorization codes and sessions
//! - Access and refresh tokens
//! - User sessions
//! - AccessPolicy resources
//!
//! # Implementations
//!
//! Storage implementations are provided in separate crates:
//!
//! - `octofhir-auth-postgres` - PostgreSQL storage backend

pub mod client;
pub mod session;

pub use client::ClientStorage;
pub use session::SessionStorage;
