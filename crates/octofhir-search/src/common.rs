//! Common search parameters that apply to all FHIR resources.
//!
//! These parameters are defined in the FHIR specification and are available
//! for all resource types. They are registered before loading package-specific
//! search parameters.

use crate::parameters::{SearchParameter, SearchParameterType};
use crate::registry::SearchParameterRegistry;

/// Register all common (Resource-level) search parameters.
///
/// These parameters are available for all resource types as defined
/// in the FHIR specification. They are registered with base "Resource"
/// so they apply universally.
pub fn register_common_parameters(registry: &SearchParameterRegistry) {
    // _id - logical id of the resource
    registry.register(
        SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.id")
        .with_description("Logical id of this artifact"),
    );

    // _lastUpdated - when the resource was last changed
    registry.register(
        SearchParameter::new(
            "_lastUpdated",
            "http://hl7.org/fhir/SearchParameter/Resource-lastUpdated",
            SearchParameterType::Date,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.lastUpdated")
        .with_description("When the resource version last changed"),
    );

    // _tag - tags applied to this resource
    registry.register(
        SearchParameter::new(
            "_tag",
            "http://hl7.org/fhir/SearchParameter/Resource-tag",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.tag")
        .with_description("Tags applied to this resource"),
    );

    // _profile - profiles this resource claims to conform to
    registry.register(
        SearchParameter::new(
            "_profile",
            "http://hl7.org/fhir/SearchParameter/Resource-profile",
            SearchParameterType::Uri,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.profile")
        .with_description("Profiles this resource claims to conform to"),
    );

    // _security - security labels applied to this resource
    registry.register(
        SearchParameter::new(
            "_security",
            "http://hl7.org/fhir/SearchParameter/Resource-security",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.security")
        .with_description("Security Labels applied to this resource"),
    );

    // _source - identifies where the resource comes from (R4+)
    registry.register(
        SearchParameter::new(
            "_source",
            "http://hl7.org/fhir/SearchParameter/Resource-source",
            SearchParameterType::Uri,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.source")
        .with_description("Identifies where the resource comes from"),
    );

    // _content - search on the entire content of the resource
    registry.register(
        SearchParameter::new(
            "_content",
            "http://hl7.org/fhir/SearchParameter/Resource-content",
            SearchParameterType::Special,
            vec!["Resource".to_string()],
        )
        .with_description("Search on the entire content of the resource"),
    );

    // _text - search on the narrative text of the resource
    registry.register(
        SearchParameter::new(
            "_text",
            "http://hl7.org/fhir/SearchParameter/DomainResource-text",
            SearchParameterType::Special,
            vec!["DomainResource".to_string()],
        )
        .with_description("Search on the narrative of the resource"),
    );

    tracing::debug!(count = 8, "Registered common search parameters");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_common_parameters() {
        let registry = SearchParameterRegistry::new();
        register_common_parameters(&registry);

        // Check that common parameters are registered
        assert!(registry.get("Patient", "_id").is_some());
        assert!(registry.get("Patient", "_lastUpdated").is_some());
        assert!(registry.get("Patient", "_tag").is_some());
        assert!(registry.get("Patient", "_profile").is_some());
        assert!(registry.get("Patient", "_security").is_some());
        assert!(registry.get("Patient", "_source").is_some());

        // Check they work for any resource type
        assert!(registry.get("Observation", "_id").is_some());
        assert!(registry.get("Condition", "_lastUpdated").is_some());
    }

    #[test]
    fn test_common_parameter_expressions() {
        let registry = SearchParameterRegistry::new();
        register_common_parameters(&registry);

        let id_param = registry.get("Patient", "_id").unwrap();
        assert_eq!(id_param.expression.as_deref(), Some("Resource.id"));

        let last_updated = registry.get("Patient", "_lastUpdated").unwrap();
        assert_eq!(
            last_updated.expression.as_deref(),
            Some("Resource.meta.lastUpdated")
        );
    }
}
