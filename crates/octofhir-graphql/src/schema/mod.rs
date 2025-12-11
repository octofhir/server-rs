//! GraphQL schema building and lazy loading.
//!
//! This module provides infrastructure for building and managing the GraphQL
//! schema. The schema is built dynamically from the FHIR model provider and
//! uses lazy initialization to avoid blocking server startup.
//!
//! ## Components
//!
//! - [`LazySchema`] - Thread-safe lazy schema holder with hot-reload support
//! - [`FhirSchemaBuilder`] - Builds GraphQL schema from FHIR model
//!
//! ## Architecture
//!
//! The schema building process:
//! 1. Server starts immediately without waiting for schema
//! 2. First GraphQL request triggers schema build
//! 3. Concurrent requests either wait or receive 503
//! 4. Schema is cached after successful build
//! 5. `invalidate()` can be called to trigger rebuild (hot-reload)

mod builder;
mod lazy;
mod resource_type;

pub use builder::{FhirSchemaBuilder, SchemaBuilderConfig};
pub use lazy::LazySchema;
pub use resource_type::{fhir_type_to_graphql, is_complex_type, is_primitive_type};
