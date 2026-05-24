use crate::ir::strategy::IndexStrategy;
use crate::parameters::{SearchModifier, SearchParameterType, SearchPrefix};
use crate::parser::ParsedParam;
use crate::sql_builder::SqlBuilderError;
use crate::types::date_ast::{DateClause, DatePredicate};

/// Top-level FHIR search expression.
///
/// Repeated query parameters become `And`; comma-separated values become `Or`.
#[derive(Debug, Clone)]
pub enum SearchExpr {
    And(Vec<SearchExpr>),
    Or(Vec<SearchExpr>),
    Not(Box<SearchExpr>),
    Param(SearchParamExpr),
}

/// SearchParameter expression after registry lookup and type parsing.
#[derive(Debug, Clone)]
pub struct SearchParamExpr {
    pub resource_type: String,
    pub code: String,
    pub search_type: SearchParameterType,
    pub modifier: Option<SearchModifier>,
    pub values: Vec<SearchValue>,
    pub expression: Option<String>,
    pub strategy_hint: Option<IndexStrategy>,
}

/// Type-specific predicate payload.
#[derive(Debug, Clone)]
pub enum SearchValue {
    Id(IdPredicate),
    Date(DatePredicate),
    String(StringPredicate),
    Token(TokenPredicate),
    Reference(ReferencePredicate),
    Number(NumberPredicate),
    Quantity(QuantityPredicate),
    Uri(UriPredicate),
    Composite(CompositePredicate),
}

/// Logical resource id predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdPredicate {
    Equals { value: String },
    Missing { is_missing: bool },
}

/// Logical id SearchParameter occurrence over the resource id column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: IdPredicate,
    pub negated: bool,
}

impl IdClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: IdPredicate::Missing { is_missing },
                negated: false,
            }]);
        }

        let negated = matches!(param.modifier, Some(SearchModifier::Not));
        if let Some(modifier) = &param.modifier
            && !negated
        {
            return Err(SqlBuilderError::InvalidModifier(format!("{modifier:?}")));
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }
            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: IdPredicate::Equals {
                    value: value.raw.clone(),
                },
                negated,
            });
        }

        Ok(clauses)
    }
}

/// Date SearchParameter occurrence.
///
/// `clauses` are OR-combined because they come from one comma-separated query
/// occurrence. Repeated occurrences are represented by multiple `DateParamExpr`
/// values under a parent `SearchExpr::And`.
#[derive(Debug, Clone)]
pub struct DateParamExpr {
    pub clauses: Vec<DateClause>,
}

impl DateParamExpr {
    pub fn new(clauses: Vec<DateClause>) -> Self {
        Self { clauses }
    }
}

/// String SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPredicate {
    /// Default FHIR string search: case/accent-insensitive starts-with.
    Prefix { value: String },
    /// `:contains`: case/accent-insensitive substring.
    Contains { value: String },
    /// `:exact`: case/accent-sensitive full-string equality.
    Exact { value: String },
    /// `:text`: full-text search over resource narrative.
    Text { value: String },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

/// String SearchParameter occurrence.
///
/// Clauses are OR-combined because they come from one comma-separated query
/// occurrence. Repeated occurrences are still represented by independent SQL
/// builder conditions and therefore AND-combined by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: StringPredicate,
}

impl StringClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: StringPredicate::Missing { is_missing },
            }]);
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let predicate = match &param.modifier {
                None => StringPredicate::Prefix {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Contains) => StringPredicate::Contains {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Exact) => StringPredicate::Exact {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Text) => StringPredicate::Text {
                    value: value.raw.clone(),
                },
                Some(other) => {
                    return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
                }
            };

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate,
            });
        }

        Ok(clauses)
    }
}

/// URI SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriPredicate {
    /// Default URI search: exact match.
    Exact { value: String },
    /// `:below`: stored URI starts with the search URI.
    Below { value: String },
    /// `:above`: search URI starts with the stored URI.
    Above { value: String },
    /// `:contains`: case-insensitive substring match.
    Contains { value: String },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

/// URI SearchParameter occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: UriPredicate,
}

impl UriClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: UriPredicate::Missing { is_missing },
            }]);
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let predicate = match &param.modifier {
                None => UriPredicate::Exact {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Below) => UriPredicate::Below {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Above) => UriPredicate::Above {
                    value: value.raw.clone(),
                },
                Some(SearchModifier::Contains) => UriPredicate::Contains {
                    value: value.raw.clone(),
                },
                Some(other) => {
                    return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
                }
            };

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate,
            });
        }

        Ok(clauses)
    }
}

/// Number SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumberPredicate {
    /// Prefix comparison over the current JSONB numeric cast fallback.
    Comparison { prefix: SearchPrefix, value: String },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

/// Number SearchParameter occurrence.
///
/// Clauses are OR-combined because they come from one comma-separated query
/// occurrence. Runtime SQL still uses the legacy JSONB numeric cast path; this
/// IR node exists to make that fallback visible in debug output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: NumberPredicate,
}

impl NumberClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: NumberPredicate::Missing { is_missing },
            }]);
        }

        if let Some(other) = &param.modifier {
            return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: NumberPredicate::Comparison {
                    prefix: value.prefix.unwrap_or(SearchPrefix::Eq),
                    value: value.raw.clone(),
                },
            });
        }

        Ok(clauses)
    }
}

/// Quantity SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuantityPredicate {
    /// Numeric quantity comparison plus optional system/code constraints.
    Comparison {
        prefix: SearchPrefix,
        value: String,
        system: Option<String>,
        code: Option<String>,
    },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

/// Quantity SearchParameter occurrence.
///
/// Runtime SQL currently uses JSONB numeric casts and direct system/code
/// checks. This node captures the FHIR shape so debug can expose it as a
/// non-index-backed fallback until a quantity sidecar exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantityClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: QuantityPredicate,
}

impl QuantityClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: QuantityPredicate::Missing { is_missing },
            }]);
        }

        if let Some(other) = &param.modifier {
            return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let (quantity_value, system, code) = parse_quantity_predicate_value(&value.raw);
            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: QuantityPredicate::Comparison {
                    prefix: value.prefix.unwrap_or(SearchPrefix::Eq),
                    value: quantity_value.to_string(),
                    system: system.map(str::to_string),
                    code: code.map(str::to_string),
                },
            });
        }

        Ok(clauses)
    }
}

fn parse_quantity_predicate_value(value: &str) -> (&str, Option<&str>, Option<&str>) {
    let parts: Vec<&str> = value.splitn(3, '|').collect();
    let quantity_value = parts[0];
    let system = parts.get(1).copied().filter(|s| !s.is_empty());
    let code = parts.get(2).copied().filter(|s| !s.is_empty());
    (quantity_value, system, code)
}

/// Component metadata for a composite SearchParameter tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeComponentSpec {
    pub code: String,
    pub search_type: SearchParameterType,
    pub expression: String,
}

/// Per-component value inside one composite tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeComponentPredicate {
    pub spec: CompositeComponentSpec,
    pub value: String,
}

/// Composite safety classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeSafety {
    /// Independent intersection is semantically safe.
    SafeIndependent,
    /// Same-element co-occurrence is required but not proven by current SQL.
    RequiresSameElement,
    /// Component shape is not supported by the current renderer.
    Unsupported,
}

/// Composite SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositePredicate {
    Tuple {
        components: Vec<CompositeComponentPredicate>,
        safety: CompositeSafety,
    },
    Missing {
        is_missing: bool,
    },
}

/// Composite SearchParameter occurrence.
///
/// Each clause is one `$`-delimited tuple from a comma-separated query value.
/// Runtime SQL still uses the existing component builder; this IR shape makes
/// the tuple semantics and co-occurrence risk explicit for debug output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: CompositePredicate,
}

impl CompositeClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
        components: &[CompositeComponentSpec],
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: CompositePredicate::Missing { is_missing },
            }]);
        }

        if let Some(other) = &param.modifier {
            return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let values = value.raw.split('$').collect::<Vec<_>>();
            if values.len() != components.len() {
                return Err(SqlBuilderError::InvalidSearchValue(format!(
                    "Composite parameter expects {} components, got {}",
                    components.len(),
                    values.len()
                )));
            }

            let component_predicates = values
                .into_iter()
                .zip(components.iter())
                .filter(|(component_value, _)| !component_value.is_empty())
                .map(|(component_value, spec)| CompositeComponentPredicate {
                    spec: spec.clone(),
                    value: component_value.to_string(),
                })
                .collect::<Vec<_>>();

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: CompositePredicate::Tuple {
                    safety: classify_composite_safety(&component_predicates),
                    components: component_predicates,
                },
            });
        }

        Ok(clauses)
    }
}

fn classify_composite_safety(components: &[CompositeComponentPredicate]) -> CompositeSafety {
    if components.is_empty() {
        return CompositeSafety::Unsupported;
    }

    let has_repeating_component = components.iter().any(|component| {
        let expression = component.spec.expression.to_ascii_lowercase();
        expression.contains("component")
            || expression.contains("coding")
            || expression.contains("identifier")
            || expression.contains("extension")
    });

    if has_repeating_component && components.len() > 1 {
        CompositeSafety::RequiresSameElement
    } else {
        CompositeSafety::SafeIndependent
    }
}

/// Token SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenPredicate {
    /// `code`: match the code regardless of system.
    AnySystemCode { code: String },
    /// `|code`: match code where system is absent.
    NoSystemCode { code: String },
    /// `system|`: match any code in the system.
    SystemAnyCode { system: String },
    /// `system|code`: match both system and code.
    SystemCode { system: String, code: String },
    /// `:of-type=system|code|value` for Identifier.
    IdentifierOfType {
        system: String,
        code: String,
        value: String,
    },
    /// Terminology-backed token set modifiers.
    TerminologySet {
        modifier: TokenSetModifier,
        value: String,
    },
    /// `:text`.
    DisplayText { text: String },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSetModifier {
    In,
    NotIn,
    Below,
    Above,
}

/// Storage shape selected by token dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenIndexShape {
    Identifier,
    SimpleCode,
    Coding,
}

/// Token SearchParameter occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: TokenPredicate,
    pub negated: bool,
    pub index_shape: TokenIndexShape,
}

impl TokenClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
        index_shape: TokenIndexShape,
    ) -> Result<Vec<Self>, SqlBuilderError> {
        if matches!(param.modifier, Some(SearchModifier::Missing)) {
            let is_missing = param
                .values
                .first()
                .map(|v| v.raw.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            return Ok(vec![Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate: TokenPredicate::Missing { is_missing },
                negated: false,
                index_shape,
            }]);
        }

        let mut clauses = Vec::new();
        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let (predicate, negated) = match &param.modifier {
                None | Some(SearchModifier::Not) => {
                    let predicate = parse_token_predicate(&value.raw);
                    (
                        predicate,
                        matches!(param.modifier, Some(SearchModifier::Not)),
                    )
                }
                Some(SearchModifier::Text) => (
                    TokenPredicate::DisplayText {
                        text: value.raw.clone(),
                    },
                    false,
                ),
                Some(SearchModifier::OfType) => (parse_identifier_of_type(&value.raw)?, false),
                Some(SearchModifier::In) => (
                    TokenPredicate::TerminologySet {
                        modifier: TokenSetModifier::In,
                        value: value.raw.clone(),
                    },
                    false,
                ),
                Some(SearchModifier::NotIn) => (
                    TokenPredicate::TerminologySet {
                        modifier: TokenSetModifier::NotIn,
                        value: value.raw.clone(),
                    },
                    false,
                ),
                Some(SearchModifier::Below) => (
                    TokenPredicate::TerminologySet {
                        modifier: TokenSetModifier::Below,
                        value: value.raw.clone(),
                    },
                    false,
                ),
                Some(SearchModifier::Above) => (
                    TokenPredicate::TerminologySet {
                        modifier: TokenSetModifier::Above,
                        value: value.raw.clone(),
                    },
                    false,
                ),
                Some(other) => {
                    return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
                }
            };

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate,
                negated,
                index_shape,
            });
        }

        Ok(clauses)
    }
}

fn parse_token_predicate(raw: &str) -> TokenPredicate {
    if let Some(pos) = raw.find('|') {
        let system = &raw[..pos];
        let code = &raw[pos + 1..];
        match (system.is_empty(), code.is_empty()) {
            (true, _) => TokenPredicate::NoSystemCode {
                code: code.to_string(),
            },
            (false, true) => TokenPredicate::SystemAnyCode {
                system: system.to_string(),
            },
            (false, false) => TokenPredicate::SystemCode {
                system: system.to_string(),
                code: code.to_string(),
            },
        }
    } else {
        TokenPredicate::AnySystemCode {
            code: raw.to_string(),
        }
    }
}

fn parse_identifier_of_type(raw: &str) -> Result<TokenPredicate, SqlBuilderError> {
    let parts: Vec<&str> = raw.splitn(3, '|').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err(SqlBuilderError::InvalidSearchValue(
            "of-type modifier requires non-empty system|code|value format".to_string(),
        ));
    }

    Ok(TokenPredicate::IdentifierOfType {
        system: parts[0].to_string(),
        code: parts[1].to_string(),
        value: parts[2].to_string(),
    })
}

/// Reference SearchParameter predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePredicate {
    /// Local reference by target id, optionally scoped to target type.
    Local {
        target_type: Option<String>,
        target_id: String,
    },
    /// External absolute URL reference.
    External { url: String },
    /// `:identifier` reference search using identifier rows in `search_idx_reference`.
    Identifier {
        system: Option<String>,
        require_no_system: bool,
        value: String,
    },
    /// `:missing=true|false`.
    Missing { is_missing: bool },
}

/// Reference SearchParameter occurrence.
///
/// Clauses are OR-combined because they come from one comma-separated query
/// occurrence. Runtime SQL still includes the legacy JSONB fallback for default
/// reference matching; this IR node exposes the sidecar intent for debug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceClause {
    pub resource_type: String,
    pub param_code: String,
    pub predicate: ReferencePredicate,
    pub target_types: Vec<String>,
    pub jsonb_fallback_value: Option<String>,
}

impl ReferenceClause {
    pub fn from_parsed_param(
        param: &ParsedParam,
        resource_type: &str,
        target_types: &[String],
    ) -> Result<Vec<Self>, SqlBuilderError> {
        let mut clauses = Vec::new();

        for value in &param.values {
            if value.raw.is_empty() {
                continue;
            }

            let predicate = match &param.modifier {
                None => parse_reference_predicate(&value.raw, target_types),
                Some(SearchModifier::Type(type_name)) => ReferencePredicate::Local {
                    target_type: Some(type_name.clone()),
                    target_id: value.raw.clone(),
                },
                Some(SearchModifier::Identifier) => parse_reference_identifier(&value.raw),
                Some(SearchModifier::Missing) => ReferencePredicate::Missing {
                    is_missing: value.raw.eq_ignore_ascii_case("true"),
                },
                Some(other) => {
                    return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
                }
            };

            clauses.push(Self {
                resource_type: resource_type.to_string(),
                param_code: param.name.clone(),
                predicate,
                target_types: target_types.to_vec(),
                jsonb_fallback_value: matches!(param.modifier, None).then(|| value.raw.clone()),
            });
        }

        Ok(clauses)
    }
}

fn parse_reference_predicate(raw: &str, target_types: &[String]) -> ReferencePredicate {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return ReferencePredicate::External {
            url: raw.to_string(),
        };
    }

    if let Some((target_type, target_id)) = raw.split_once('/') {
        return ReferencePredicate::Local {
            target_type: Some(target_type.to_string()),
            target_id: target_id.to_string(),
        };
    }

    ReferencePredicate::Local {
        target_type: (target_types.len() == 1).then(|| target_types[0].clone()),
        target_id: raw.to_string(),
    }
}

fn parse_reference_identifier(raw: &str) -> ReferencePredicate {
    if let Some((system, value)) = raw.split_once('|') {
        ReferencePredicate::Identifier {
            system: (!system.is_empty()).then(|| system.to_string()),
            require_no_system: system.is_empty(),
            value: value.to_string(),
        }
    } else {
        ReferencePredicate::Identifier {
            system: None,
            require_no_system: false,
            value: raw.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;

    fn parsed(raw: &str) -> ParsedParam {
        ParsedParam {
            name: "code".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: raw.to_string(),
            }],
        }
    }

    #[test]
    fn token_ir_preserves_fhir_syntax_distinctions() {
        let cases = [
            (
                "8480-6",
                TokenPredicate::AnySystemCode {
                    code: "8480-6".to_string(),
                },
            ),
            (
                "|8480-6",
                TokenPredicate::NoSystemCode {
                    code: "8480-6".to_string(),
                },
            ),
            (
                "http://loinc.org|",
                TokenPredicate::SystemAnyCode {
                    system: "http://loinc.org".to_string(),
                },
            ),
            (
                "http://loinc.org|8480-6",
                TokenPredicate::SystemCode {
                    system: "http://loinc.org".to_string(),
                    code: "8480-6".to_string(),
                },
            ),
        ];

        for (raw, expected) in cases {
            let clauses = TokenClause::from_parsed_param(
                &parsed(raw),
                "Observation",
                TokenIndexShape::Coding,
            )
            .unwrap();
            assert_eq!(clauses.len(), 1);
            assert_eq!(clauses[0].predicate, expected);
        }
    }
}
