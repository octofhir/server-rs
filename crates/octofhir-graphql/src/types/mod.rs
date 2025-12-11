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

mod scalars;

pub use scalars::{
    FhirBase64Binary, FhirCanonical, FhirDate, FhirDateTime, FhirDecimal, FhirId, FhirInstant,
    FhirMarkdown, FhirOid, FhirPositiveInt, FhirTime, FhirUnsignedInt, FhirUri, FhirUrl, FhirUuid,
    FhirXhtml,
};
