use serde::{Deserialize, Serialize};
use std::fmt;

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
