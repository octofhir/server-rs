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
        }
    }

    /// Set the FHIRPath expression.
    #[must_use]
    pub fn with_expression(mut self, expr: impl Into<String>) -> Self {
        self.expression = Some(expr.into());
        self
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
