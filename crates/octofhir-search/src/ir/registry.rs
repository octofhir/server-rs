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

/// Resource-level SearchParameter backed by a physical resource table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceColumnParam {
    Id,
    LastUpdated,
    /// `_profile`, served from the generated `profile` `text[]` column rather
    /// than by expanding `meta.profile` out of the resource JSONB per row.
    Profile,
}

impl ResourceColumnParam {
    /// Physical column name on the resource table.
    pub fn column_name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::LastUpdated => "updated_at",
            Self::Profile => "profile",
        }
    }
}

/// Resolve common FHIR Resource SearchParameters that are stored as row columns.
pub fn resolve_resource_column_param(param_def: &SearchParameter) -> Option<ResourceColumnParam> {
    // IGs may publish the standard parameter under their own canonical URL.
    // Preserve its semantics using code, type and expression together; URL
    // equality alone misclassifies those definitions as generic Coding tokens.
    let expression = param_def.expression.as_deref()?;
    let path = match expression {
        "id" | "meta.lastUpdated" | "meta.profile" => expression,
        _ => {
            let (owner, path) = expression.split_once('.')?;
            if owner != "Resource"
                && owner != "DomainResource"
                && !param_def.base.iter().any(|base| base == owner)
            {
                return None;
            }
            path
        }
    };
    match (param_def.code.as_str(), param_def.param_type, path) {
        ("_id", SearchParameterType::Token, "id") => Some(ResourceColumnParam::Id),
        ("_lastUpdated", SearchParameterType::Date, "meta.lastUpdated") => {
            Some(ResourceColumnParam::LastUpdated)
        }
        ("_profile", SearchParameterType::Uri, "meta.profile") => {
            Some(ResourceColumnParam::Profile)
        }
        _ => None,
    }
}

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
                // Use the referenced sub-param's own full FHIRPath
                // (e.g. `Observation.component.value.ofType(Quantity)`), not the
                // composite's base-relative fragment (`value.ofType(Quantity)`).
                // The fragment alone resolves against the resource root and
                // wrongly navigates top-level `valueQuantity` instead of
                // `component[*].valueQuantity` — and, lacking the `component`
                // prefix, also defeats the RequiresSameElement classification.
                expression: resolved
                    .expression
                    .clone()
                    .unwrap_or_else(|| component.expression.clone()),
                element_type_hint: resolved.element_type_hint.clone(),
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
        // The composite references a sub-param by canonical url; the resolved
        // spec must carry that sub-param's OWN full expression (component-scoped),
        // not the composite's base-relative fragment.
        registry.register(
            SearchParameter::new(
                "component-code",
                "http://hl7.org/fhir/SearchParameter/Observation-component-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.component.code"),
        );
        registry.register(
            SearchParameter::new(
                "combo",
                "http://example.org/SearchParameter/Observation-combo",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_components(vec![crate::parameters::SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-component-code"
                    .to_string(),
                expression: "code".to_string(),
            }]),
        );

        let combo = registry
            .get_by_url("http://example.org/SearchParameter/Observation-combo")
            .unwrap();
        let specs = resolve_composite_component_specs(&registry, "Observation", &combo).unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].code, "component-code");
        assert_eq!(specs[0].search_type, SearchParameterType::Token);
        assert_eq!(specs[0].expression, "Observation.component.code");
    }

    #[test]
    fn resolves_common_resource_columns_from_search_parameter_metadata() {
        let id = SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.id");
        let last_updated = SearchParameter::new(
            "_lastUpdated",
            "http://hl7.org/fhir/SearchParameter/Resource-lastUpdated",
            SearchParameterType::Date,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.lastUpdated");
        let json_backed = SearchParameter::new(
            "_tag",
            "http://hl7.org/fhir/SearchParameter/Resource-tag",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.meta.tag");

        assert_eq!(
            resolve_resource_column_param(&id),
            Some(ResourceColumnParam::Id)
        );
        assert_eq!(
            resolve_resource_column_param(&last_updated),
            Some(ResourceColumnParam::LastUpdated)
        );
        assert_eq!(resolve_resource_column_param(&json_backed), None);
    }

    #[test]
    fn standard_resource_parameters_keep_column_semantics_with_ig_canonicals() {
        for (code, param_type, expression, expected) in [
            (
                "_id",
                SearchParameterType::Token,
                "Resource.id",
                ResourceColumnParam::Id,
            ),
            (
                "_lastUpdated",
                SearchParameterType::Date,
                "Resource.meta.lastUpdated",
                ResourceColumnParam::LastUpdated,
            ),
            (
                "_profile",
                SearchParameterType::Uri,
                "Resource.meta.profile",
                ResourceColumnParam::Profile,
            ),
        ] {
            let param = SearchParameter::new(
                code,
                format!("https://ig.example/SearchParameter/Patient-{code}"),
                param_type,
                vec!["Patient".into()],
            )
            .with_expression(expression);
            assert_eq!(resolve_resource_column_param(&param), Some(expected));
            let unqualified = param
                .clone()
                .with_expression(expression.trim_start_matches("Resource."));
            assert_eq!(resolve_resource_column_param(&unqualified), Some(expected));
            let concrete = param
                .clone()
                .with_expression(expression.replace("Resource.", "Patient."));
            assert_eq!(resolve_resource_column_param(&concrete), Some(expected));
            let unrelated = param
                .clone()
                .with_expression(expression.replace("Resource.", "Practitioner."));
            assert_eq!(resolve_resource_column_param(&unrelated), None);
            assert_eq!(
                resolve_resource_column_param(&param.with_expression("Patient.identifier")),
                None
            );
        }
    }
}
