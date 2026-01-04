//! App Platform module for declarative application manifests.
//!
//! This module provides types and utilities for defining FHIR applications
//! using a declarative manifest format. Apps can specify:
//! - Custom operations with routing and policies
//! - Pre-created resources (Client, AccessPolicy, etc.)
//! - Subscriptions for events and notifications

pub mod types;

pub use types::*;
