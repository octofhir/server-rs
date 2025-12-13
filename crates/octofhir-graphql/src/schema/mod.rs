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
//! - [`FhirTypeGenerator`] - Generates GraphQL types from FHIR schema
//! - [`InputTypeGenerator`] - Generates input types for mutations
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
mod input_types;
mod lazy;
mod resource_type;
mod type_generator;

pub use builder::{DynModelProvider, FhirSchemaBuilder, SchemaBuilderConfig};
pub use input_types::{
    create_json_scalar, create_operation_outcome_issue_type, create_operation_outcome_type,
    InputTypeGenerator,
};
pub use lazy::LazySchema;
pub use resource_type::{fhir_type_to_graphql, is_complex_type, is_primitive_type};
pub use type_generator::{FhirTypeGenerator, TypeRegistry};
