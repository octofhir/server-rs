use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// FHIR R4B SearchParameter type enumeration
/// See: https://hl7.org/fhir/R4B/search.html#table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchParameterType {
    Number,
    Date,
    String,
    Token,
    Reference,
    Composite,
    Quantity,
    Uri,
    Special,
}

impl SearchParameterType {
    /// Parse a search parameter type from a string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "number" => Some(Self::Number),
            "date" => Some(Self::Date),
            "string" => Some(Self::String),
            "token" => Some(Self::Token),
            "reference" => Some(Self::Reference),
            "composite" => Some(Self::Composite),
            "quantity" => Some(Self::Quantity),
            "uri" => Some(Self::Uri),
            "special" => Some(Self::Special),
            _ => None,
        }
    }
}

/// Supported search modifiers (subset per FHIR R4B)
/// Applied as suffix to parameter name: `name:modifier`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchModifier {
    Exact,
    Contains,
    Text,
    In,
    NotIn,
    Below,
    Above,
    Not,
    Identifier,   // for reference parameters
    Type(String), // e.g., subject:Patient
    Missing,      // value should be boolean (handled during parsing)
    OfType,       // for token parameters
}

impl SearchModifier {
    /// Parse a search modifier from a string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "missing" => Some(Self::Missing),
            "exact" => Some(Self::Exact),
            "contains" => Some(Self::Contains),
            "not" => Some(Self::Not),
            "text" => Some(Self::Text),
            "in" => Some(Self::In),
            "not-in" => Some(Self::NotIn),
            "below" => Some(Self::Below),
            "above" => Some(Self::Above),
            "identifier" => Some(Self::Identifier),
            "ofType" => Some(Self::OfType),
            // Type modifier is handled separately during parsing
            _ => None,
        }
    }

    /// Check if this modifier is applicable to the given parameter type.
    pub fn applicable_to(&self, param_type: &SearchParameterType) -> bool {
        match self {
            Self::Missing => true, // All types support :missing
            Self::Exact | Self::Contains => {
                matches!(param_type, SearchParameterType::String)
            }
            Self::Not | Self::Text | Self::In | Self::NotIn => {
                matches!(param_type, SearchParameterType::Token)
            }
            Self::Below | Self::Above => {
                matches!(
                    param_type,
                    SearchParameterType::Token | SearchParameterType::Uri
                )
            }
            Self::Type(_) | Self::Identifier => {
                matches!(param_type, SearchParameterType::Reference)
            }
            Self::OfType => matches!(param_type, SearchParameterType::Token),
        }
    }
}

/// Prefixes for number/date search values
/// e.g., `ge2020-01-01`, `lt5.0`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchPrefix {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Sa, // starts after
    Eb, // ends before
    Ap, // approximately
}

impl fmt::Display for SearchPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SearchPrefix::Eq => "eq",
            SearchPrefix::Ne => "ne",
            SearchPrefix::Gt => "gt",
            SearchPrefix::Lt => "lt",
            SearchPrefix::Ge => "ge",
            SearchPrefix::Le => "le",
            SearchPrefix::Sa => "sa",
            SearchPrefix::Eb => "eb",
            SearchPrefix::Ap => "ap",
        };
        f.write_str(s)
    }
}

impl SearchPrefix {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "eq" => Some(Self::Eq),
            "ne" => Some(Self::Ne),
            "gt" => Some(Self::Gt),
            "lt" => Some(Self::Lt),
            "ge" => Some(Self::Ge),
            "le" => Some(Self::Le),
            "sa" => Some(Self::Sa),
            "eb" => Some(Self::Eb),
            "ap" => Some(Self::Ap),
            _ => None,
        }
    }
}

/// A single search parameter definition (metadata)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParameterDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: SearchParameterType,
    pub description: Option<String>,
}

impl SearchParameterDefinition {
    pub fn new<N: Into<String>>(name: N, kind: SearchParameterType) -> Self {
        Self {
            name: name.into(),
            kind,
            description: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Container for parameter definitions
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParameters {
    pub params: Vec<SearchParameterDefinition>,
}

impl SearchParameters {
    pub fn new() -> Self {
        Self { params: Vec::new() }
    }

    pub fn with_param(mut self, def: SearchParameterDefinition) -> Self {
        self.params.push(def);
        self
    }
}

/// Component of a composite search parameter.
#[derive(Debug, Clone)]
pub struct SearchParameterComponent {
    /// URL of the component SearchParameter definition
    pub definition: String,
    /// FHIRPath expression for this component
    pub expression: String,
}

/// A complete search parameter definition loaded from FHIR packages.
///
/// This represents a FHIR SearchParameter resource with all fields needed
/// for search execution and validation.
#[derive(Debug, Clone)]
pub struct SearchParameter {
    /// The code used in search queries (e.g., "name", "identifier")
    pub code: String,
    /// The canonical URL of this search parameter
    pub url: String,
    /// The type of search parameter (token, string, reference, etc.)
    pub param_type: SearchParameterType,
    /// FHIRPath expression for extracting values
    pub expression: Option<String>,
    /// XPath expression (legacy, for reference)
    pub xpath: Option<String>,
    /// Resource types this parameter applies to
    pub base: Vec<String>,
    /// Target resource types for reference parameters
    pub target: Vec<String>,
    /// Supported modifiers for this parameter
    pub modifier: Vec<SearchModifier>,
    /// Human-readable description
    pub description: String,
    /// Components for composite search parameters
    pub component: Vec<SearchParameterComponent>,
    /// Cached JSONB path segments derived from FHIRPath expression.
    /// Pre-computed once when parameter is created to avoid repeated parsing.
    /// Format: segments like ["name", "family"] derived from "Patient.name.family"
    cached_jsonb_path: Option<Vec<String>>,
}

impl SearchParameter {
    /// Create a new search parameter with required fields.
    pub fn new(
        code: impl Into<String>,
        url: impl Into<String>,
        param_type: SearchParameterType,
        base: Vec<String>,
    ) -> Self {
        Self {
            code: code.into(),
            url: url.into(),
            param_type,
            expression: None,
            xpath: None,
            base,
            target: Vec::new(),
            modifier: Vec::new(),
            description: String::new(),
            component: Vec::new(),
            cached_jsonb_path: None,
        }
    }

    /// Set the FHIRPath expression and pre-compute the JSONB path.
    #[must_use]
    pub fn with_expression(mut self, expr: impl Into<String>) -> Self {
        let expr_str = expr.into();
        // Pre-compute JSONB path for efficiency
        self.cached_jsonb_path = Some(Self::compute_jsonb_path(&expr_str));
        self.expression = Some(expr_str);
        self
    }

    /// Compute JSONB path segments from a FHIRPath expression.
    ///
    /// Converts expressions like "Patient.name.family" to ["name", "family"].
    fn compute_jsonb_path(expression: &str) -> Vec<String> {
        // Remove common resource type prefixes
        let expr = expression
            .split('.')
            .skip(1) // Skip resource type (Patient, Resource, etc.)
            .collect::<Vec<_>>()
            .join(".");

        if expr.is_empty() {
            return Vec::new();
        }

        // Split by '.' and handle special cases
        expr.split('.')
            .filter(|s| !s.is_empty())
            .map(|s| {
                // Handle array access like "name[0]"
                if let Some(base) = s.strip_suffix(']')
                    && let Some((name, _idx)) = base.split_once('[')
                {
                    return name.to_string();
                }
                s.to_string()
            })
            .collect()
    }

    /// Get the cached JSONB path, or compute it on-demand if not cached.
    ///
    /// For parameters created with `with_expression()`, this returns the pre-computed path.
    /// For parameters with expression set directly, this computes and returns the path.
    pub fn jsonb_path(&self) -> Option<Vec<String>> {
        if let Some(ref cached) = self.cached_jsonb_path {
            return Some(cached.clone());
        }
        self.expression
            .as_ref()
            .map(|e| Self::compute_jsonb_path(e))
    }

    /// Get the cached JSONB path for a specific resource type.
    ///
    /// This strips the resource type prefix if it matches, providing the correct
    /// path segments for the JSONB query.
    pub fn jsonb_path_for_type(&self, resource_type: &str) -> Option<Vec<String>> {
        let expr = self.expression.as_ref()?;

        // Check if expression starts with specific resource type
        let prefixes = [
            format!("{}.", resource_type),
            "Resource.".to_string(),
            "DomainResource.".to_string(),
        ];

        for prefix in &prefixes {
            if let Some(stripped) = expr.strip_prefix(prefix) {
                return Some(
                    stripped
                        .split('.')
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            if let Some(base) = s.strip_suffix(']')
                                && let Some((name, _idx)) = base.split_once('[')
                            {
                                return name.to_string();
                            }
                            s.to_string()
                        })
                        .collect(),
                );
            }
        }

        // Fallback to cached path
        self.jsonb_path()
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set target resource types.
    #[must_use]
    pub fn with_targets(mut self, targets: Vec<String>) -> Self {
        self.target = targets;
        self
    }

    /// Set supported modifiers.
    #[must_use]
    pub fn with_modifiers(mut self, modifiers: Vec<SearchModifier>) -> Self {
        self.modifier = modifiers;
        self
    }

    /// Set components for composite search parameters.
    #[must_use]
    pub fn with_components(mut self, components: Vec<SearchParameterComponent>) -> Self {
        self.component = components;
        self
    }

    /// Check if this parameter applies to a given resource type.
    pub fn applies_to(&self, resource_type: &str) -> bool {
        self.base
            .iter()
            .any(|b| b == resource_type || b == "Resource" || b == "DomainResource")
    }

    /// Check if this is a common parameter (applies to all resources).
    pub fn is_common(&self) -> bool {
        self.base
            .iter()
            .any(|b| b == "Resource" || b == "DomainResource")
    }

    /// Get this parameter as an Arc for shared ownership.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonb_path_caching() {
        let param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family");

        // Cached path should be available immediately
        let path = param.jsonb_path();
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["name", "family"]);
    }

    #[test]
    fn test_jsonb_path_for_type() {
        let param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family");

        let path = param.jsonb_path_for_type("Patient");
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["name", "family"]);
    }

    #[test]
    fn test_jsonb_path_resource_common() {
        let param = SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.id");

        let path = param.jsonb_path_for_type("Patient");
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["id"]);

        let path = param.jsonb_path_for_type("Observation");
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["id"]);
    }

    #[test]
    fn test_jsonb_path_with_array_index() {
        let param = SearchParameter::new(
            "given",
            "http://hl7.org/fhir/SearchParameter/Patient-given",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name[0].given");

        let path = param.jsonb_path();
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec!["name", "given"]);
    }

    #[test]
    fn test_jsonb_path_no_expression() {
        let param = SearchParameter::new(
            "_content",
            "http://hl7.org/fhir/SearchParameter/Resource-content",
            SearchParameterType::Special,
            vec!["Resource".to_string()],
        );

        let path = param.jsonb_path();
        assert!(path.is_none());
    }

    #[test]
    fn test_modifier_applicable_to() {
        // String modifiers
        assert!(SearchModifier::Exact.applicable_to(&SearchParameterType::String));
        assert!(SearchModifier::Contains.applicable_to(&SearchParameterType::String));
        assert!(!SearchModifier::Exact.applicable_to(&SearchParameterType::Token));

        // Token modifiers
        assert!(SearchModifier::In.applicable_to(&SearchParameterType::Token));
        assert!(SearchModifier::NotIn.applicable_to(&SearchParameterType::Token));
        assert!(!SearchModifier::In.applicable_to(&SearchParameterType::String));

        // :missing works for all
        assert!(SearchModifier::Missing.applicable_to(&SearchParameterType::String));
        assert!(SearchModifier::Missing.applicable_to(&SearchParameterType::Token));
        assert!(SearchModifier::Missing.applicable_to(&SearchParameterType::Date));

        // :below/:above work for token and uri
        assert!(SearchModifier::Below.applicable_to(&SearchParameterType::Token));
        assert!(SearchModifier::Below.applicable_to(&SearchParameterType::Uri));
        assert!(!SearchModifier::Below.applicable_to(&SearchParameterType::String));
    }
}
