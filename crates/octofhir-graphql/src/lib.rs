//! # octofhir-graphql
//!
//! GraphQL API layer for the OctoFHIR FHIR server.
//!
//! This crate provides a GraphQL interface to FHIR resources following the
//! GraphQL for FHIR specification. It supports:
//!
//! - Query operations for reading and searching FHIR resources
//! - Mutation operations for creating, updating, and deleting resources
//! - Subscription support for real-time updates
//! - FHIR-specific scalar types for primitive values
//! - DataLoader pattern for efficient N+1 query resolution
//!
//! ## Overview
//!
//! The GraphQL schema is dynamically generated from the FHIR model provider,
//! supporting all FHIR resource types and their relationships. The schema
//! uses lazy initialization to avoid blocking server startup.
//!
//! ## Endpoints
//!
//! - `POST /$graphql` - System-level GraphQL endpoint
//! - `GET /$graphql` - System-level GraphQL (query via URL param)
//! - `POST /:type/:id/$graphql` - Instance-level GraphQL endpoint
//!
//! ## Configuration
//!
//! Add to `octofhir.toml`:
//!
//! ```toml
//! [graphql]
//! enabled = true
//! max_depth = 15
//! max_complexity = 500
//! introspection = true
//! playground = true
//! ```
//!
//! ## Modules
//!
//! - [`config`] - Configuration options
//! - [`types`] - FHIR custom scalar types for GraphQL
//! - [`schema`] - Schema building and lazy loading
//! - [`context`] - GraphQL execution context
//! - [`handler`] - Axum HTTP handlers
//! - [`error`] - Error types for GraphQL operations

pub mod config;
pub mod context;
pub mod error;
pub mod handler;
pub mod resolvers;
pub mod schema;
pub mod types;

// Re-export main types
pub use config::GraphQLConfig;
pub use context::{GraphQLContext, GraphQLContextBuilder};
pub use error::GraphQLError;
pub use handler::{graphql_handler, graphql_handler_get, instance_graphql_handler};
pub use schema::{FhirSchemaBuilder, LazySchema, SchemaBuilderConfig};

/// Result type for GraphQL operations.
pub type Result<T> = std::result::Result<T, GraphQLError>;
