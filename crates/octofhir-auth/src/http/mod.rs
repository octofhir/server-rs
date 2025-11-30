//! HTTP handlers for OAuth 2.0 endpoints.
//!
//! This module provides Axum handlers for OAuth endpoints.
//!
//! # Available Handlers
//!
//! - [`revoke`] - Token revocation endpoint (RFC 7009)
//! - [`introspect`] - Token introspection endpoint (RFC 7662)
//! - [`launch`] - SMART on FHIR launch context creation

pub mod introspect;
pub mod launch;
pub mod revoke;

pub use introspect::introspect_handler;
pub use launch::{CreateLaunchRequest, CreateLaunchResponse, LaunchState, create_launch_handler};
pub use revoke::revoke_handler;
