//! FHIR custom scalar types for GraphQL.
//!
//! This module provides GraphQL scalar implementations for FHIR primitive types.
//! Each scalar handles parsing (input) and serialization (output) of FHIR values.
//!
//! ## FHIR Primitive Types
//!
//! FHIR defines several primitive types that don't directly map to GraphQL's
//! built-in scalars. This module provides custom scalar implementations for:
//!
//! - Date/Time: `FhirInstant`, `FhirDateTime`, `FhirDate`, `FhirTime`
//! - Identifiers: `FhirUri`, `FhirUrl`, `FhirCanonical`, `FhirOid`, `FhirUuid`, `FhirId`
//! - Numbers: `FhirPositiveInt`, `FhirUnsignedInt`, `FhirDecimal`
//! - Other: `FhirBase64Binary`, `FhirMarkdown`, `FhirXhtml`
//!
//! ## Reference Type
//!
//! The module also provides the FHIR Reference type with lazy resource resolution:
//!
//! - `create_reference_type`: Creates the Reference GraphQL type
//! - `create_all_resources_union`: Creates the AllResources union for polymorphic resolution

mod reference;
mod scalars;

pub use reference::{create_all_resources_union, create_reference_type};
pub use scalars::{
    FhirBase64Binary, FhirCanonical, FhirDate, FhirDateTime, FhirDecimal, FhirId, FhirInstant,
    FhirMarkdown, FhirOid, FhirPositiveInt, FhirTime, FhirUnsignedInt, FhirUri, FhirUrl, FhirUuid,
    FhirXhtml,
};
