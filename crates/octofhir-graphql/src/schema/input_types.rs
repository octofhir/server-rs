//! Input types for GraphQL mutations.
//!
//! This module provides input type generation for FHIR resource mutations.
//! Input types accept resource data as JSON for flexibility and compatibility
//! with FHIR's complex nested structures.

use async_graphql::dynamic::{InputObject, InputValue, TypeRef};
use tracing::{debug, trace};

/// Generates input types for FHIR resource mutations.
///
/// For each resource type, this generates:
/// - `{Resource}Input` - Input type accepting the resource JSON
///
/// ## Design Decision
///
/// FHIR resources have deeply nested structures with extensions, choice types
/// (value[x]), and contained resources. Rather than generating complex input
/// object hierarchies that mirror the full FHIR schema, we use a JSON scalar
/// approach where the `resource` field accepts the full FHIR resource as JSON.
///
/// This approach:
/// - Simplifies the GraphQL schema
/// - Allows full FHIR validation to happen server-side
/// - Supports all FHIR features without schema bloat
/// - Matches common FHIR GraphQL implementation patterns
pub struct InputTypeGenerator;

impl InputTypeGenerator {
    /// Creates an input type for a specific resource type.
    ///
    /// The input type has a single `resource` field accepting JSON.
    ///
    /// Example generated type:
    /// ```graphql
    /// input PatientInput {
    ///   resource: JSON!
    /// }
    /// ```
    pub fn create_resource_input(resource_type: &str) -> InputObject {
        let type_name = format!("{}Input", resource_type);

        trace!(resource_type, type_name = %type_name, "Creating resource input type");

        InputObject::new(&type_name)
            .description(format!(
                "Input type for creating/updating {} resources. The resource field accepts the full FHIR {} resource as JSON.",
                resource_type, resource_type
            ))
            .field(
                InputValue::new("resource", TypeRef::named_nn("JSON"))
                    .description(format!(
                        "The {} resource as JSON. Must be a valid FHIR {} resource.",
                        resource_type, resource_type
                    ))
            )
    }

    /// Creates input types for all provided resource types.
    ///
    /// Returns a vector of (type_name, InputObject) tuples.
    pub fn create_all_inputs(resource_types: &[String]) -> Vec<(String, InputObject)> {
        debug!(count = resource_types.len(), "Generating resource input types");

        resource_types
            .iter()
            .map(|rt| {
                let input = Self::create_resource_input(rt);
                (format!("{}Input", rt), input)
            })
            .collect()
    }
}

/// Creates the JSON scalar type for accepting arbitrary JSON input.
///
/// This scalar accepts any valid JSON value and serializes it as-is.
pub fn create_json_scalar() -> async_graphql::dynamic::Scalar {
    async_graphql::dynamic::Scalar::new("JSON")
        .description("A JSON scalar value. Accepts any valid JSON.")
}

/// Creates the OperationOutcome type for mutation responses.
///
/// This type represents FHIR OperationOutcome which is returned for:
/// - Validation errors
/// - Operation failures
/// - Delete confirmations
pub fn create_operation_outcome_type() -> async_graphql::dynamic::Object {
    use async_graphql::dynamic::{Field, FieldFuture, Object};
    use async_graphql::Value;

    // OperationOutcomeIssue type
    let issue = Object::new("OperationOutcomeIssue")
        .description("A single issue in an OperationOutcome")
        .field(
            Field::new("severity", TypeRef::named_nn(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "severity")
                })
            })
            .description("Severity of the issue: fatal | error | warning | information"),
        )
        .field(
            Field::new("code", TypeRef::named_nn(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "code")
                })
            })
            .description("Error or warning code"),
        )
        .field(
            Field::new("diagnostics", TypeRef::named(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "diagnostics")
                })
            })
            .description("Additional diagnostic information"),
        )
        .field(
            Field::new("location", TypeRef::named_list(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "location")
                })
            })
            .description("FHIRPath of element(s) related to the issue"),
        )
        .field(
            Field::new("expression", TypeRef::named_list(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "expression")
                })
            })
            .description("FHIRPath expression of element(s) related to the issue"),
        );

    // OperationOutcome type
    Object::new("OperationOutcome")
        .description("Information about the outcome of an operation, particularly errors and warnings")
        .field(
            Field::new("resourceType", TypeRef::named_nn(TypeRef::STRING), |_| {
                FieldFuture::new(async move {
                    Ok(Some(Value::String("OperationOutcome".to_string())))
                })
            })
            .description("Resource type (always 'OperationOutcome')"),
        )
        .field(
            Field::new("id", TypeRef::named(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "id")
                })
            })
            .description("Logical id of this outcome"),
        )
        .field(
            Field::new("issue", TypeRef::named_nn_list_nn("OperationOutcomeIssue"), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "issue")
                })
            })
            .description("Issues that occurred during the operation"),
        );

    issue
}

/// Creates the OperationOutcomeIssue type.
pub fn create_operation_outcome_issue_type() -> async_graphql::dynamic::Object {
    use async_graphql::dynamic::{Field, FieldFuture, Object};

    Object::new("OperationOutcomeIssue")
        .description("A single issue in an OperationOutcome")
        .field(
            Field::new("severity", TypeRef::named_nn(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "severity")
                })
            })
            .description("Severity of the issue: fatal | error | warning | information"),
        )
        .field(
            Field::new("code", TypeRef::named_nn(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "code")
                })
            })
            .description("Error or warning code"),
        )
        .field(
            Field::new("diagnostics", TypeRef::named(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "diagnostics")
                })
            })
            .description("Additional diagnostic information"),
        )
        .field(
            Field::new("location", TypeRef::named_list(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "location")
                })
            })
            .description("FHIRPath of element(s) related to the issue"),
        )
        .field(
            Field::new("expression", TypeRef::named_list(TypeRef::STRING), |ctx| {
                FieldFuture::new(async move {
                    extract_field(&ctx, "expression")
                })
            })
            .description("FHIRPath expression of element(s) related to the issue"),
        )
}

/// Helper to extract a field from parent JSON value.
fn extract_field(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
    field_name: &str,
) -> Result<Option<async_graphql::Value>, async_graphql::Error> {
    use async_graphql::Value;

    if let Some(parent) = ctx.parent_value.as_value()
        && let Value::Object(obj) = parent
        && let Some(value) = obj.get(&async_graphql::Name::new(field_name))
    {
        return Ok(Some(value.clone()));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_resource_input() {
        let input = InputTypeGenerator::create_resource_input("Patient");
        assert_eq!(input.type_name(), "PatientInput");
    }

    #[test]
    fn test_create_all_inputs() {
        let types = vec!["Patient".to_string(), "Observation".to_string()];
        let inputs = InputTypeGenerator::create_all_inputs(&types);

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].0, "PatientInput");
        assert_eq!(inputs[1].0, "ObservationInput");
    }

    #[test]
    fn test_create_json_scalar() {
        let scalar = create_json_scalar();
        assert_eq!(scalar.type_name(), "JSON");
    }
}
