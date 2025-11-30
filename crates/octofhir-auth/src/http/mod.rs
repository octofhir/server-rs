//! HTTP handlers for OAuth 2.0 endpoints.
//!
//! This module provides Axum handlers for OAuth endpoints.
//!
//! # Available Handlers
//!
//! - [`revoke`] - Token revocation endpoint (RFC 7009)
//! - [`introspect`] - Token introspection endpoint (RFC 7662)
//! - [`launch`] - SMART on FHIR launch context creation
//! - [`discovery`] - SMART configuration endpoint
//! - [`jwks`] - JWKS endpoint (RFC 7517)
//! - [`userinfo`] - OpenID Connect UserInfo endpoint

pub mod discovery;
pub mod introspect;
pub mod jwks;
pub mod launch;
pub mod revoke;
pub mod userinfo;

pub use discovery::{SmartConfigState, smart_configuration_handler};
pub use introspect::introspect_handler;
pub use jwks::{JwksState, jwks_handler};
pub use launch::{CreateLaunchRequest, CreateLaunchResponse, LaunchState, create_launch_handler};
pub use revoke::revoke_handler;
pub use userinfo::{UserInfoResponse, userinfo_handler};
