//! Registry-backed SearchParameter resolution for typed IR construction.
//!
//! This layer is the boundary between FHIR SearchParameter metadata and the
//! typed IR. It must use registry metadata, not expression-string heuristics,
//! for semantic type decisions.

use crate::ir::ast::CompositeComponentSpec;
use crate::parameters::{SearchParameter, SearchParameterType};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::SqlBuilderError;
use std::sync::Arc;

/// Resolve one SearchParameter component definition through the registry.
///
/// FHIR composite components normally reference canonical SearchParameter URLs.
/// Custom/internal definitions may use a local code; support that as an
/// explicit fallback while still requiring a registry entry.
pub fn resolve_component_definition(
    registry: &SearchParameterRegistry,
    resource_type: &str,
    definition: &str,
) -> Result<Arc<SearchParameter>, SqlBuilderError> {
    registry
        .get_by_url(definition)
        .or_else(|| registry.get(resource_type, definition))
        .ok_or_else(|| {
            SqlBuilderError::InvalidPath(format!(
                "Composite component definition not found in search registry: {definition}"
            ))
        })
}

/// Resolve a composite SearchParameter into typed component specs.
pub fn resolve_composite_component_specs(
    registry: &SearchParameterRegistry,
    resource_type: &str,
    param_def: &SearchParameter,
) -> Result<Vec<CompositeComponentSpec>, SqlBuilderError> {
    param_def
        .component
        .iter()
        .map(|component| {
            let resolved =
                resolve_component_definition(registry, resource_type, &component.definition)?;
            Ok(CompositeComponentSpec {
                code: resolved.code.clone(),
                search_type: resolved.param_type,
                expression: component.expression.clone(),
            })
        })
        .collect()
}

/// Stable lowercase SearchParameter type name for legacy component renderers.
pub fn search_type_name(search_type: SearchParameterType) -> &'static str {
    match search_type {
        SearchParameterType::Number => "number",
        SearchParameterType::Date => "date",
        SearchParameterType::String => "string",
        SearchParameterType::Token => "token",
        SearchParameterType::Reference => "reference",
        SearchParameterType::Composite => "composite",
        SearchParameterType::Quantity => "quantity",
        SearchParameterType::Uri => "uri",
        SearchParameterType::Special => "special",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_composite_component_types_from_registry() {
        let registry = SearchParameterRegistry::new();
        registry.register(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code"),
        );
        registry.register(
            SearchParameter::new(
                "combo",
                "http://example.org/SearchParameter/Observation-combo",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_components(vec![crate::parameters::SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                expression: "Observation.component.code".to_string(),
            }]),
        );

        let combo = registry
            .get_by_url("http://example.org/SearchParameter/Observation-combo")
            .unwrap();
        let specs = resolve_composite_component_specs(&registry, "Observation", &combo).unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].code, "code");
        assert_eq!(specs[0].search_type, SearchParameterType::Token);
        assert_eq!(specs[0].expression, "Observation.component.code");
    }
}
