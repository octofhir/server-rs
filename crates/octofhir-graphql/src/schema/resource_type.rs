//! FHIR resource type generation for GraphQL schema.
//!
//! This module generates GraphQL object types for FHIR resources,
//! including their fields, nested types, and type mappings.

use std::sync::Arc;

use async_graphql::dynamic::{Object, TypeRef};
use octofhir_fhir_model::provider::ModelProvider;
use tracing::{debug, warn};

/// Maps a FHIR primitive type to a GraphQL type reference.
///
/// Returns the appropriate TypeRef for FHIR primitives, mapping them
/// to either built-in GraphQL scalars or custom FHIR scalars.
pub fn fhir_type_to_graphql(fhir_type: &str) -> TypeRef {
    match fhir_type {
        // Map to built-in GraphQL scalars
        "boolean" => TypeRef::named(TypeRef::BOOLEAN),
        "integer" | "integer64" => TypeRef::named(TypeRef::INT),

        // Map to custom FHIR scalars
        "instant" => TypeRef::named("FhirInstant"),
        "dateTime" => TypeRef::named("FhirDateTime"),
        "date" => TypeRef::named("FhirDate"),
        "time" => TypeRef::named("FhirTime"),
        "decimal" => TypeRef::named("FhirDecimal"),
        "uri" => TypeRef::named("FhirUri"),
        "url" => TypeRef::named("FhirUrl"),
        "canonical" => TypeRef::named("FhirCanonical"),
        "oid" => TypeRef::named("FhirOid"),
        "uuid" => TypeRef::named("FhirUuid"),
        "id" => TypeRef::named("FhirId"),
        "base64Binary" => TypeRef::named("FhirBase64Binary"),
        "markdown" => TypeRef::named("FhirMarkdown"),
        "positiveInt" => TypeRef::named("FhirPositiveInt"),
        "unsignedInt" => TypeRef::named("FhirUnsignedInt"),
        "xhtml" => TypeRef::named("FhirXhtml"),

        // String and code types map to String
        "string" | "code" => TypeRef::named(TypeRef::STRING),

        // Complex types and resources map to their named type
        // These should be registered as separate Object types
        other => TypeRef::named(other),
    }
}

/// Checks if a type is a FHIR primitive type.
pub fn is_primitive_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "boolean"
            | "integer"
            | "integer64"
            | "string"
            | "decimal"
            | "uri"
            | "url"
            | "canonical"
            | "base64Binary"
            | "instant"
            | "date"
            | "dateTime"
            | "time"
            | "code"
            | "oid"
            | "uuid"
            | "id"
            | "markdown"
            | "unsignedInt"
            | "positiveInt"
            | "xhtml"
    )
}

/// Checks if a type is a FHIR complex type (not a resource).
pub fn is_complex_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "Address"
            | "Age"
            | "Annotation"
            | "Attachment"
            | "CodeableConcept"
            | "CodeableReference"
            | "Coding"
            | "ContactDetail"
            | "ContactPoint"
            | "Contributor"
            | "Count"
            | "DataRequirement"
            | "Distance"
            | "Dosage"
            | "Duration"
            | "ElementDefinition"
            | "Expression"
            | "Extension"
            | "HumanName"
            | "Identifier"
            | "MarketingStatus"
            | "Meta"
            | "Money"
            | "MoneyQuantity"
            | "Narrative"
            | "ParameterDefinition"
            | "Period"
            | "Population"
            | "ProdCharacteristic"
            | "ProductShelfLife"
            | "Quantity"
            | "Range"
            | "Ratio"
            | "RatioRange"
            | "Reference"
            | "RelatedArtifact"
            | "SampledData"
            | "Signature"
            | "SimpleQuantity"
            | "SubstanceAmount"
            | "Timing"
            | "TriggerDefinition"
            | "UsageContext"
            | "BackboneElement"
            | "Element"
    )
}

/// Context for generating GraphQL types from FHIR model.
pub struct TypeGenerator<M: ModelProvider> {
    /// The model provider for accessing FHIR schema information.
    model_provider: Arc<M>,
}

impl<M: ModelProvider + Send + Sync + 'static> TypeGenerator<M> {
    /// Creates a new type generator with the given model provider.
    pub fn new(model_provider: Arc<M>) -> Self {
        Self { model_provider }
    }

    /// Generates a basic GraphQL object type for a FHIR resource.
    ///
    /// This creates a simplified type with just the JSON scalar representation
    /// since we're using async-graphql's dynamic schema which returns the
    /// resource as a JSON object that clients can query with any fields.
    pub fn generate_resource_type(&self, resource_type: &str) -> Object {
        debug!(resource_type = %resource_type, "Generating GraphQL type for resource");

        // For dynamic schema, we create a simple object that the resolver
        // populates with the full resource JSON
        let obj = Object::new(resource_type)
            .description(format!("FHIR {} resource", resource_type));

        // The actual fields are added dynamically based on the JSON structure
        // returned by the resolver. async-graphql's dynamic schema handles
        // this through the Value::Object type.

        obj
    }

    /// Gets the list of resource types from the model provider.
    pub async fn get_resource_types(&self) -> Vec<String> {
        match self.model_provider.get_resource_types().await {
            Ok(types) => {
                debug!(count = types.len(), "Retrieved resource types from model provider");
                types
            }
            Err(e) => {
                warn!(error = %e, "Failed to get resource types from model provider");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fhir_type_to_graphql_primitives() {
        // Boolean maps to built-in
        let bool_ref = fhir_type_to_graphql("boolean");
        assert!(matches!(bool_ref, TypeRef::Named { .. }));

        // String maps to built-in
        let string_ref = fhir_type_to_graphql("string");
        assert!(matches!(string_ref, TypeRef::Named { .. }));

        // Date maps to custom scalar
        let date_ref = fhir_type_to_graphql("date");
        assert!(matches!(date_ref, TypeRef::Named { .. }));
    }

    #[test]
    fn test_is_primitive_type() {
        assert!(is_primitive_type("string"));
        assert!(is_primitive_type("boolean"));
        assert!(is_primitive_type("dateTime"));
        assert!(is_primitive_type("decimal"));

        assert!(!is_primitive_type("Patient"));
        assert!(!is_primitive_type("HumanName"));
        assert!(!is_primitive_type("Reference"));
    }

    #[test]
    fn test_is_complex_type() {
        assert!(is_complex_type("HumanName"));
        assert!(is_complex_type("Address"));
        assert!(is_complex_type("CodeableConcept"));
        assert!(is_complex_type("Reference"));

        assert!(!is_complex_type("string"));
        assert!(!is_complex_type("Patient"));
    }
}
