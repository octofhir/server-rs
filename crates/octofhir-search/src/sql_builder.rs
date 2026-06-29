//! SQL Builder for generating PostgreSQL JSONB queries.
//!
//! This module provides a builder pattern for constructing parameterized
//! SQL queries that search within JSONB resource data.
//!
//! ## Features
//!
//! - **Fluent API**: Chain method calls to build complex queries
//! - **Type-safe paths**: JSONB path abstraction with validation
//! - **Parameterized queries**: All user input via bind parameters
//! - **Aggregation support**: COUNT, _summary=count, _total=accurate
//! - **Include support**: _include and _revinclude for related resources
//! - **Chaining support**: JOINs for chained search parameters

use std::fmt;
use thiserror::Error;

use crate::parameters::ElementTypeHint;

/// Errors that can occur during SQL building.
#[derive(Debug, Error)]
pub enum SqlBuilderError {
    #[error("Invalid modifier '{0}' for parameter type")]
    InvalidModifier(String),

    #[error("Invalid search value: {0}")]
    InvalidSearchValue(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Invalid JSON path: {0}")]
    InvalidPath(String),

    #[error("Invalid identifier: {0}")]
    InvalidIdentifier(String),

    #[error("Query too complex: {0}")]
    QueryTooComplex(String),
}

// ============================================================================
// JSONB Path Abstraction
// ============================================================================

/// A type-safe JSONB path with validation and escaping.
///
/// Prevents SQL injection by validating all path segments are alphanumeric
/// with underscores only.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonbPath {
    segments: Vec<String>,
    is_array_element: bool,
}

impl JsonbPath {
    /// Create a new JSONB path from segments.
    ///
    /// Returns an error if any segment contains invalid characters.
    pub fn new(segments: Vec<String>) -> Result<Self, SqlBuilderError> {
        for segment in &segments {
            validate_identifier(segment)?;
        }
        Ok(Self {
            segments,
            is_array_element: false,
        })
    }

    /// Create a path that represents an array element access.
    pub fn array_element(segments: Vec<String>) -> Result<Self, SqlBuilderError> {
        let mut path = Self::new(segments)?;
        path.is_array_element = true;
        Ok(path)
    }

    /// Get the path segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Check if this path represents array element access.
    pub fn is_array_element(&self) -> bool {
        self.is_array_element
    }

    /// Build JSONB accessor string for this path.
    pub fn to_accessor(&self, resource_col: &str, as_text: bool) -> String {
        build_jsonb_accessor(resource_col, &self.segments, as_text)
    }

    /// Build JSONB accessor for array element access.
    ///
    /// Returns a tuple of (array_path, element_alias) for use in
    /// `jsonb_array_elements(array_path) AS element_alias`.
    pub fn to_array_accessor(&self, resource_col: &str) -> (String, String) {
        if self.segments.is_empty() {
            return (resource_col.to_string(), "elem".to_string());
        }

        // All but last segment form the array path
        let array_segments = if self.segments.len() > 1 {
            &self.segments[..self.segments.len() - 1]
        } else {
            &self.segments[..]
        };

        let array_path = build_jsonb_accessor(resource_col, array_segments, false);
        (array_path, "elem".to_string())
    }
}

/// Validate an identifier (table name, column name, path segment).
///
/// Only allows alphanumeric characters and underscores.
fn validate_identifier(name: &str) -> Result<(), SqlBuilderError> {
    if name.is_empty() {
        return Err(SqlBuilderError::InvalidIdentifier(
            "Empty identifier".to_string(),
        ));
    }

    // Allow alphanumeric, underscore, and also common FHIR patterns like brackets for array access
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '[' || c == ']')
    {
        return Err(SqlBuilderError::InvalidIdentifier(name.to_string()));
    }

    Ok(())
}

/// Escape a PostgreSQL identifier (table name, column name).
pub fn escape_identifier(name: &str) -> Result<String, SqlBuilderError> {
    validate_identifier(name)?;
    Ok(format!("\"{name}\""))
}

// ============================================================================
// Search Condition Types
// ============================================================================

/// Comparison operators for search conditions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operator {
    /// Equal (=)
    Eq,
    /// Not equal (!=)
    Ne,
    /// Greater than (>)
    Gt,
    /// Less than (<)
    Lt,
    /// Greater than or equal (>=)
    Ge,
    /// Less than or equal (<=)
    Le,
    /// LIKE pattern match
    Like,
    /// Case-insensitive LIKE
    ILike,
    /// JSONB contains (@>)
    Contains,
    /// JSONB contained by (<@)
    ContainedBy,
    /// Array overlaps (&&)
    Overlaps,
    /// IS NULL check
    IsNull,
    /// IS NOT NULL check
    IsNotNull,
}

impl Operator {
    /// Get the SQL operator string.
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Like => "LIKE",
            Self::ILike => "ILIKE",
            Self::Contains => "@>",
            Self::ContainedBy => "<@",
            Self::Overlaps => "&&",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }
}

/// A search condition that can be combined with other conditions.
#[derive(Debug, Clone)]
pub enum SearchCondition {
    /// Simple comparison: path op value
    Simple {
        path: JsonbPath,
        op: Operator,
        value: SqlValue,
    },

    /// Raw SQL condition with parameters
    Raw { sql: String, params: Vec<SqlValue> },

    /// Array element search using jsonb_array_elements
    Array {
        path: JsonbPath,
        element_condition: Box<SearchCondition>,
    },

    /// EXISTS subquery
    Exists { path: JsonbPath, exists: bool },

    /// Combine conditions with OR
    Or(Vec<SearchCondition>),

    /// Combine conditions with AND
    And(Vec<SearchCondition>),

    /// Negation of a condition
    Not(Box<SearchCondition>),

    /// Always true (used for empty OR lists)
    True,

    /// Always false (used for empty AND lists)
    False,
}

impl SearchCondition {
    /// Create a simple comparison condition.
    pub fn simple(path: JsonbPath, op: Operator, value: SqlValue) -> Self {
        Self::Simple { path, op, value }
    }

    /// Create an OR condition.
    pub fn or(conditions: Vec<SearchCondition>) -> Self {
        if conditions.is_empty() {
            Self::False
        } else if conditions.len() == 1 {
            conditions.into_iter().next().unwrap()
        } else {
            Self::Or(conditions)
        }
    }

    /// Create an AND condition.
    pub fn and(conditions: Vec<SearchCondition>) -> Self {
        if conditions.is_empty() {
            Self::True
        } else if conditions.len() == 1 {
            conditions.into_iter().next().unwrap()
        } else {
            Self::And(conditions)
        }
    }

    /// Create a NOT condition.
    pub fn negate(condition: SearchCondition) -> Self {
        Self::Not(Box::new(condition))
    }

    /// Create a raw SQL condition.
    pub fn raw(sql: impl Into<String>, params: Vec<SqlValue>) -> Self {
        Self::Raw {
            sql: sql.into(),
            params,
        }
    }
}

/// SQL value types for parameterized queries.
#[derive(Debug, Clone)]
pub enum SqlValue {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Json(String),
    Timestamp(String),
    Null,
}

impl SqlValue {
    /// Get the value as a string for display/debugging.
    pub fn as_display_str(&self) -> String {
        match self {
            Self::Text(s) | Self::Json(s) | Self::Timestamp(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Boolean(b) => b.to_string(),
            Self::Null => "NULL".to_string(),
        }
    }
}

// ============================================================================
// Sort and Pagination
// ============================================================================

/// Sort order for query results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// Sort specification.
#[derive(Debug, Clone)]
pub struct SortSpec {
    pub path: Option<JsonbPath>,
    pub column: Option<String>,
    pub order: SortOrder,
    pub nulls_last: bool,
}

impl SortSpec {
    pub fn new(path: JsonbPath, order: SortOrder) -> Self {
        Self {
            path: Some(path),
            column: None,
            order,
            nulls_last: true,
        }
    }

    pub fn asc(path: JsonbPath) -> Self {
        Self::new(path, SortOrder::Asc)
    }

    pub fn desc(path: JsonbPath) -> Self {
        Self::new(path, SortOrder::Desc)
    }

    pub fn column(column: impl Into<String>, order: SortOrder) -> Result<Self, SqlBuilderError> {
        let column = column.into();
        validate_identifier(&column)?;
        Ok(Self {
            path: None,
            column: Some(column),
            order,
            nulls_last: true,
        })
    }
}

/// Pagination settings.
#[derive(Debug, Clone, Default)]
pub struct Pagination {
    pub limit: Option<usize>,
    pub offset: usize,
}

impl Pagination {
    pub fn new(limit: usize, offset: usize) -> Self {
        Self {
            limit: Some(limit),
            offset,
        }
    }
}

// ============================================================================
// Include/RevInclude Specifications
// ============================================================================

/// _include specification for forward references.
#[derive(Debug, Clone)]
pub struct IncludeSpec {
    /// Source resource type (e.g., "Observation")
    pub source_type: String,
    /// Search parameter name (e.g., "subject")
    pub param_name: String,
    /// Target resource type (e.g., "Patient"), None means any
    pub target_type: Option<String>,
    /// Whether this is :iterate
    pub iterate: bool,
}

impl IncludeSpec {
    pub fn new(source_type: impl Into<String>, param_name: impl Into<String>) -> Self {
        Self {
            source_type: source_type.into(),
            param_name: param_name.into(),
            target_type: None,
            iterate: false,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target_type = Some(target.into());
        self
    }

    pub fn iterate(mut self) -> Self {
        self.iterate = true;
        self
    }
}

/// _revinclude specification for reverse references.
#[derive(Debug, Clone)]
pub struct RevIncludeSpec {
    /// Referencing resource type (e.g., "Observation")
    pub source_type: String,
    /// Search parameter name that references target (e.g., "subject")
    pub param_name: String,
    /// Target resource type being referenced (e.g., "Patient")
    pub target_type: Option<String>,
    /// Whether this is :iterate
    pub iterate: bool,
}

impl RevIncludeSpec {
    pub fn new(source_type: impl Into<String>, param_name: impl Into<String>) -> Self {
        Self {
            source_type: source_type.into(),
            param_name: param_name.into(),
            target_type: None,
            iterate: false,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target_type = Some(target.into());
        self
    }

    pub fn iterate(mut self) -> Self {
        self.iterate = true;
        self
    }
}

// ============================================================================
// Chain Join for Chained Searches
// ============================================================================

/// A JOIN specification for chained search parameters.
#[derive(Debug, Clone)]
pub struct ChainJoin {
    /// Source resource type (e.g., "Observation")
    pub from_resource: String,
    /// Target resource type (e.g., "Patient")
    pub to_resource: String,
    /// Path to reference field in source
    pub reference_path: JsonbPath,
    /// Table alias for the joined table
    pub alias: String,
}

impl ChainJoin {
    pub fn new(
        from_resource: impl Into<String>,
        to_resource: impl Into<String>,
        reference_path: JsonbPath,
        alias: impl Into<String>,
    ) -> Self {
        Self {
            from_resource: from_resource.into(),
            to_resource: to_resource.into(),
            reference_path,
            alias: alias.into(),
        }
    }
}

// ============================================================================
// Query Output Mode
// ============================================================================

/// What the query should return.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryMode {
    /// Return full resources
    Resources,
    /// Return only count (_summary=count)
    Count,
    /// Return resources with total count (_total=accurate)
    ResourcesWithTotal,
    /// Return IDs only
    IdsOnly,
}

// ============================================================================
// FHIR Query Builder
// ============================================================================

/// Maximum number of conditions allowed to prevent DoS
const MAX_CONDITIONS: usize = 100;

/// Maximum number of JOINs allowed
const MAX_JOINS: usize = 10;

/// Fluent builder for constructing FHIR search SQL queries.
///
/// # Example
///
/// ```ignore
/// let query = FhirQueryBuilder::new("Patient", "public")
///     .where_condition(SearchCondition::simple(
///         JsonbPath::new(vec!["name".into(), "family".into()])?,
///         Operator::ILike,
///         SqlValue::Text("%smith%".into()),
///     ))
///     .sort_by(SortSpec::asc(JsonbPath::new(vec!["name".into(), "family".into()])?))
///     .paginate(10, 0)
///     .build()?;
/// ```
#[derive(Debug, Clone)]
pub struct FhirQueryBuilder {
    resource_type: String,
    schema: String,
    conditions: Vec<SearchCondition>,
    includes: Vec<IncludeSpec>,
    revincludes: Vec<RevIncludeSpec>,
    sort: Vec<SortSpec>,
    pagination: Pagination,
    chain_joins: Vec<ChainJoin>,
    mode: QueryMode,
    table_alias: Option<String>,
    /// When true, emit `resource::text` instead of `resource` in SELECT.
    /// This avoids JSONB -> Value deserialization for raw string output.
    raw_resource: bool,
}

impl FhirQueryBuilder {
    /// Create a new query builder for a resource type.
    pub fn new(resource_type: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            schema: schema.into(),
            conditions: Vec::new(),
            includes: Vec::new(),
            revincludes: Vec::new(),
            sort: Vec::new(),
            pagination: Pagination::default(),
            chain_joins: Vec::new(),
            mode: QueryMode::Resources,
            table_alias: None,
            raw_resource: false,
        }
    }

    /// Set a table alias for the main resource table.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.table_alias = Some(alias.into());
        self
    }

    /// Emit `resource::text` instead of `resource` in SELECT for raw string output.
    pub fn with_raw_resource(mut self, raw: bool) -> Self {
        self.raw_resource = raw;
        self
    }

    /// Add a search condition.
    pub fn where_condition(mut self, condition: SearchCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Add multiple search conditions (AND semantics).
    pub fn where_conditions(mut self, conditions: Vec<SearchCondition>) -> Self {
        self.conditions.extend(conditions);
        self
    }

    /// Add an _include specification.
    pub fn include(mut self, spec: IncludeSpec) -> Self {
        self.includes.push(spec);
        self
    }

    /// Add a _revinclude specification.
    pub fn revinclude(mut self, spec: RevIncludeSpec) -> Self {
        self.revincludes.push(spec);
        self
    }

    /// Add a sort specification.
    pub fn sort_by(mut self, spec: SortSpec) -> Self {
        self.sort.push(spec);
        self
    }

    /// Set pagination (limit and offset).
    pub fn paginate(mut self, limit: usize, offset: usize) -> Self {
        self.pagination = Pagination::new(limit, offset);
        self
    }

    /// Add a chain join for chained search.
    pub fn chain_join(mut self, join: ChainJoin) -> Self {
        self.chain_joins.push(join);
        self
    }

    /// Set query mode to count only.
    pub fn count_only(mut self) -> Self {
        self.mode = QueryMode::Count;
        self
    }

    /// Set query mode to return resources with total count.
    pub fn with_total(mut self) -> Self {
        self.mode = QueryMode::ResourcesWithTotal;
        self
    }

    /// Set query mode to return IDs only.
    pub fn ids_only(mut self) -> Self {
        self.mode = QueryMode::IdsOnly;
        self
    }

    /// Get the resource type.
    pub fn resource_type(&self) -> &str {
        &self.resource_type
    }

    /// Validate the query complexity.
    fn validate(&self) -> Result<(), SqlBuilderError> {
        if self.conditions.len() > MAX_CONDITIONS {
            return Err(SqlBuilderError::QueryTooComplex(format!(
                "Too many conditions: {} (max {})",
                self.conditions.len(),
                MAX_CONDITIONS
            )));
        }

        if self.chain_joins.len() > MAX_JOINS {
            return Err(SqlBuilderError::QueryTooComplex(format!(
                "Too many JOINs: {} (max {})",
                self.chain_joins.len(),
                MAX_JOINS
            )));
        }

        Ok(())
    }

    /// Build the SQL query and parameters.
    pub fn build(&self) -> Result<BuiltQuery, SqlBuilderError> {
        self.validate()?;

        let mut params = Vec::new();
        let table_name = self.resource_type.to_lowercase();
        let table = escape_identifier(&table_name)?;
        let schema = escape_identifier(&self.schema)?;
        let full_table = format!("{schema}.{table}");

        let alias = self.table_alias.as_deref().unwrap_or("r");
        let resource_col = format!("{alias}.resource");

        // Build SELECT clause
        let resource_select = if self.raw_resource {
            format!("{alias}.resource::text")
        } else {
            format!("{alias}.resource")
        };
        let select_clause = match self.mode {
            QueryMode::Count => "SELECT COUNT(*) as total".to_string(),
            QueryMode::IdsOnly => format!("SELECT {alias}.id"),
            QueryMode::Resources | QueryMode::ResourcesWithTotal => {
                format!(
                    "{resource_select}, {alias}.id, {alias}.txid, {alias}.created_at, {alias}.updated_at"
                )
            }
        };

        // Build FROM clause with JOINs
        let from_clause = self.build_from_clause(&full_table, alias)?;

        // Build WHERE clause
        let where_clause = self.build_where_clause(&resource_col, &mut params)?;

        // Build ORDER BY clause
        let order_clause = self.build_order_clause(&resource_col, alias)?;

        // Build LIMIT/OFFSET clause
        let limit_clause = self.build_limit_clause();

        // Combine into full query
        let mut sql = format!("SELECT {select_clause} FROM {from_clause}");

        if let Some(where_sql) = where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        if !order_clause.is_empty() && self.mode != QueryMode::Count {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order_clause);
        }

        if !limit_clause.is_empty() && self.mode != QueryMode::Count {
            sql.push(' ');
            sql.push_str(&limit_clause);
        }

        Ok(BuiltQuery { sql, params })
    }

    /// Build COUNT query for _total=accurate.
    pub fn build_count(&self) -> Result<BuiltQuery, SqlBuilderError> {
        self.validate()?;

        let mut params = Vec::new();
        let table_name = self.resource_type.to_lowercase();
        let table = escape_identifier(&table_name)?;
        let schema = escape_identifier(&self.schema)?;
        let full_table = format!("{schema}.{table}");

        let alias = self.table_alias.as_deref().unwrap_or("r");
        let resource_col = format!("{alias}.resource");

        let from_clause = self.build_from_clause(&full_table, alias)?;
        let where_clause = self.build_where_clause(&resource_col, &mut params)?;

        let mut sql = format!("SELECT COUNT(*) as total FROM {from_clause}");

        if let Some(where_sql) = where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        Ok(BuiltQuery { sql, params })
    }

    /// Build EXPLAIN ANALYZE query for performance debugging.
    pub fn build_explain(&self) -> Result<BuiltQuery, SqlBuilderError> {
        let query = self.build()?;
        Ok(BuiltQuery {
            sql: format!("EXPLAIN ANALYZE {}", query.sql),
            params: query.params,
        })
    }

    /// Extract just the bind parameters from accumulated conditions,
    /// without generating SQL. Used by query cache on cache hit.
    pub fn extract_params(&self) -> Vec<SqlValue> {
        let mut params = Vec::new();
        for condition in &self.conditions {
            Self::collect_condition_params(condition, &mut params);
        }
        params
    }

    fn collect_condition_params(condition: &SearchCondition, params: &mut Vec<SqlValue>) {
        match condition {
            SearchCondition::Simple { op, value, .. } => match op {
                Operator::IsNull | Operator::IsNotNull => {}
                _ => params.push(value.clone()),
            },
            SearchCondition::Raw { params: p, .. } => {
                params.extend(p.iter().cloned());
            }
            SearchCondition::Array {
                element_condition, ..
            } => {
                Self::collect_condition_params(element_condition, params);
            }
            SearchCondition::Exists { .. } | SearchCondition::True | SearchCondition::False => {}
            SearchCondition::Or(conditions) | SearchCondition::And(conditions) => {
                for c in conditions {
                    Self::collect_condition_params(c, params);
                }
            }
            SearchCondition::Not(inner) => {
                Self::collect_condition_params(inner, params);
            }
        }
    }

    fn build_from_clause(&self, full_table: &str, alias: &str) -> Result<String, SqlBuilderError> {
        if self.chain_joins.is_empty() {
            return Ok(format!("{full_table} AS {alias}"));
        }

        let mut from = format!("{full_table} AS {alias}");

        for join in &self.chain_joins {
            let join_table_name = join.to_resource.to_lowercase();
            let join_table = escape_identifier(&join_table_name)?;
            let join_alias = escape_identifier(&join.alias)?;
            let schema = escape_identifier(&self.schema)?;

            // Build the reference path accessor
            let ref_path = join
                .reference_path
                .to_accessor(&format!("{alias}.resource"), true);

            from.push_str(&format!(
                " INNER JOIN {schema}.{join_table} AS {join_alias} ON ({ref_path}) = CONCAT('{}/', {join_alias}.id::text)",
                join.to_resource
            ));
        }

        Ok(from)
    }

    fn build_where_clause(
        &self,
        resource_col: &str,
        params: &mut Vec<SqlValue>,
    ) -> Result<Option<String>, SqlBuilderError> {
        if self.conditions.is_empty() {
            return Ok(None);
        }

        let condition_sqls: Vec<String> = self
            .conditions
            .iter()
            .map(|c| Self::condition_to_sql(c, resource_col, params))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(condition_sqls.join(" AND ")))
    }

    fn condition_to_sql(
        condition: &SearchCondition,
        resource_col: &str,
        params: &mut Vec<SqlValue>,
    ) -> Result<String, SqlBuilderError> {
        match condition {
            SearchCondition::Simple { path, op, value } => {
                let accessor = path.to_accessor(resource_col, true);

                match op {
                    Operator::IsNull => Ok(format!("({accessor} IS NULL)")),
                    Operator::IsNotNull => Ok(format!("({accessor} IS NOT NULL)")),
                    _ => {
                        params.push(value.clone());
                        let param_num = params.len();
                        Ok(format!("({accessor} {} ${})", op.as_sql(), param_num))
                    }
                }
            }

            SearchCondition::Raw { sql, params: p } => {
                let start_param = params.len();
                params.extend(p.clone());

                // Renumber the Raw block's local placeholders ($1, $2, …) by the
                // current offset. A naive string-replace is unsafe here: replacing
                // "$1" also matches the "$1" inside "$12", and a freshly written
                // "$12" can be re-corrupted by a later "$1" pass. Match every
                // `$<digits>` once, in a single regression-free pass.
                static PLACEHOLDER_RE: std::sync::LazyLock<regex::Regex> =
                    std::sync::LazyLock::new(|| regex::Regex::new(r"\$(\d+)").unwrap());
                let result = PLACEHOLDER_RE.replace_all(sql, |caps: &regex::Captures| {
                    let n: usize = caps[1].parse().unwrap_or(0);
                    format!("${}", start_param + n)
                });
                Ok(result.into_owned())
            }

            SearchCondition::Array {
                path,
                element_condition,
            } => {
                let (array_path, elem_alias) = path.to_array_accessor(resource_col);
                let elem_sql = Self::condition_to_sql(element_condition, &elem_alias, params)?;

                Ok(format!(
                    "(EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS {elem_alias} WHERE {elem_sql}))"
                ))
            }

            SearchCondition::Exists { path, exists } => {
                let accessor = path.to_accessor(resource_col, false);
                if *exists {
                    Ok(format!("({accessor} IS NOT NULL)"))
                } else {
                    Ok(format!("({accessor} IS NULL)"))
                }
            }

            SearchCondition::Or(conditions) => {
                if conditions.is_empty() {
                    return Ok("FALSE".to_string());
                }
                let parts: Vec<String> = conditions
                    .iter()
                    .map(|c| Self::condition_to_sql(c, resource_col, params))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("({})", parts.join(" OR ")))
            }

            SearchCondition::And(conditions) => {
                if conditions.is_empty() {
                    return Ok("TRUE".to_string());
                }
                let parts: Vec<String> = conditions
                    .iter()
                    .map(|c| Self::condition_to_sql(c, resource_col, params))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("({})", parts.join(" AND ")))
            }

            SearchCondition::Not(condition) => {
                let inner = Self::condition_to_sql(condition, resource_col, params)?;
                Ok(format!("(NOT {inner})"))
            }

            SearchCondition::True => Ok("TRUE".to_string()),
            SearchCondition::False => Ok("FALSE".to_string()),
        }
    }

    fn build_order_clause(
        &self,
        resource_col: &str,
        alias: &str,
    ) -> Result<String, SqlBuilderError> {
        if self.sort.is_empty() {
            return Ok(String::new());
        }

        let parts = self
            .sort
            .iter()
            .map(|s| {
                let accessor = if let Some(column) = &s.column {
                    format!(
                        "{}.{}",
                        escape_identifier(alias)?,
                        escape_identifier(column)?
                    )
                } else if let Some(path) = &s.path {
                    path.to_accessor(resource_col, true)
                } else {
                    return Err(SqlBuilderError::InvalidPath(
                        "SortSpec has neither column nor JSONB path".to_string(),
                    ));
                };
                let nulls = if s.nulls_last { " NULLS LAST" } else { "" };
                Ok(format!("{accessor} {}{nulls}", s.order.as_sql()))
            })
            .collect::<Result<Vec<_>, SqlBuilderError>>()?;

        Ok(parts.join(", "))
    }

    fn build_limit_clause(&self) -> String {
        let mut clause = String::new();

        if let Some(limit) = self.pagination.limit {
            clause.push_str(&format!("LIMIT {limit}"));
        }

        if self.pagination.offset > 0 {
            if !clause.is_empty() {
                clause.push(' ');
            }
            clause.push_str(&format!("OFFSET {}", self.pagination.offset));
        }

        clause
    }
}

/// A built SQL query with parameters.
#[derive(Debug, Clone)]
pub struct BuiltQuery {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl fmt::Display for BuiltQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.sql)
    }
}

/// A builder for constructing SQL WHERE clauses with parameterized values.
///
/// The builder accumulates conditions and parameters, then generates
/// a complete WHERE clause with numbered parameter placeholders ($1, $2, etc.).
#[derive(Debug, Default)]
pub struct SqlBuilder {
    conditions: Vec<crate::ir::sql::SqlExpr>,
    params: Vec<SqlParam>,
    resource_col: String,
    param_offset: usize,
}

/// A SQL parameter value with its type information.
#[derive(Debug, Clone)]
pub enum SqlParam {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Json(String),
    Timestamp(String),
}

impl SqlParam {
    /// Get the value as a string for binding.
    pub fn as_str(&self) -> String {
        match self {
            Self::Text(s) | Self::Json(s) | Self::Timestamp(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Boolean(b) => b.to_string(),
        }
    }
}

impl SqlBuilder {
    /// Create a new SQL builder with the default resource column name.
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
            resource_col: "resource".to_string(),
            param_offset: 0,
        }
    }

    /// Create a new SQL builder with a custom resource column name.
    pub fn with_resource_column(column: impl Into<String>) -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
            resource_col: column.into(),
            param_offset: 0,
        }
    }

    /// Set the parameter offset for numbering (useful when combining with other queries).
    pub fn with_param_offset(mut self, offset: usize) -> Self {
        self.param_offset = offset;
        self
    }

    /// Get the resource column name.
    pub fn resource_column(&self) -> &str {
        &self.resource_col
    }

    /// Derive the id column from the resource column. `resource_col` is
    /// always of the form `<alias>.resource`; the corresponding id column is
    /// `<alias>.id`. Falls back to `"r.id"` when the suffix is missing
    /// (legacy callers / tests built with `SqlBuilder::new()`).
    pub fn id_column(&self) -> String {
        match self.resource_col.strip_suffix(".resource") {
            Some(prefix) => format!("{prefix}.id"),
            None => "r.id".to_string(),
        }
    }

    /// Add a structured SQL condition to the accumulator.
    pub fn add_condition(&mut self, condition: crate::ir::sql::SqlExpr) {
        self.conditions.push(condition);
    }

    /// Add a raw SQL string condition (escape hatch for opaque/literal SQL).
    pub fn add_raw_condition(&mut self, sql: impl Into<String>) {
        self.conditions
            .push(crate::ir::sql::SqlExpr::Raw(sql.into()));
    }

    /// Add a text parameter and return its placeholder number.
    pub fn add_text_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Text(value.into()));
        self.param_offset + self.params.len()
    }

    /// Add a JSON parameter and return its placeholder number.
    pub fn add_json_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Json(value.into()));
        self.param_offset + self.params.len()
    }

    /// Add a float parameter and return its placeholder number.
    pub fn add_float_param(&mut self, value: f64) -> usize {
        self.params.push(SqlParam::Float(value));
        self.param_offset + self.params.len()
    }

    /// Add a timestamp parameter and return its placeholder number.
    pub fn add_timestamp_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Timestamp(value.into()));
        self.param_offset + self.params.len()
    }

    /// Add an integer parameter and return its placeholder number.
    pub fn add_integer_param(&mut self, value: i64) -> usize {
        self.params.push(SqlParam::Integer(value));
        self.param_offset + self.params.len()
    }

    /// Add a boolean parameter and return its placeholder number.
    pub fn add_boolean_param(&mut self, value: bool) -> usize {
        self.params.push(SqlParam::Boolean(value));
        self.param_offset + self.params.len()
    }

    /// Get the current parameter count.
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Get all parameters.
    pub fn params(&self) -> &[SqlParam] {
        &self.params
    }

    /// Get all conditions.
    pub fn conditions(&self) -> &[crate::ir::sql::SqlExpr] {
        &self.conditions
    }

    /// Build the final WHERE clause by joining conditions with AND.
    ///
    /// Returns `None` if there are no conditions.
    pub fn build_where_clause(&self) -> Option<String> {
        if self.conditions.is_empty() {
            return None;
        }

        // Render each top-level condition independently and join with AND. This
        // matches the historical string-join behaviour exactly (no surrounding
        // parentheses around the top-level conjunction).
        Some(
            self.conditions
                .iter()
                .map(crate::ir::render::render_sql_expr)
                .collect::<Vec<_>>()
                .join(" AND "),
        )
    }

    /// Build a WHERE clause for OR conditions within a group.
    ///
    /// This is useful for comma-separated values (OR semantics within same parameter).
    pub fn build_or_clause(conditions: &[String]) -> String {
        if conditions.len() == 1 {
            conditions[0].clone()
        } else {
            format!("({})", conditions.join(" OR "))
        }
    }
}

/// Helper to convert a FHIRPath expression to a JSONB path.
///
/// This is a simplified conversion that handles common patterns.
/// More complex paths may need the full FHIRPath engine.
///
/// Handles FHIRPath union expressions like:
/// - `Patient.birthDate | Person.birthDate | RelatedPerson.birthDate`
/// - `(ActivityDefinition.useContext.value as CodeableConcept)`
///
/// Derive the JSONB property path for a SearchParameter FHIRPath expression.
///
/// Parses the expression with the real FHIRPath parser (octofhir-fhirpath) and
/// walks the AST — robustly handling unions (`|`), polymorphic casts
/// (`value.ofType(Quantity)` / `value as Quantity` -> `valueQuantity`), filters
/// (`where(...)`, `resolve()`), and index access. Returns an empty path if the
/// expression fails to parse (no string-surgery fallback — the AST is the source
/// of truth).
/// Memo for [`fhirpath_to_jsonb_path`]. The FHIRPath `parse_expression` (a
/// chumsky parser-combinator pass) is the dominant on-CPU cost on the search
/// path, and the search-parameter expressions are a fixed, finite catalog parsed
/// identically on every request. Keyed by `(resource_type, expression)`; bounded
/// by the catalog size (low thousands of tiny `Vec<String>`), so this is a
/// static-grammar memo, not per-request data caching.
static JSONB_PATH_MEMO: std::sync::LazyLock<dashmap::DashMap<(String, String), Vec<String>>> =
    std::sync::LazyLock::new(dashmap::DashMap::new);

pub fn fhirpath_to_jsonb_path(expression: &str, resource_type: &str) -> Vec<String> {
    if let Some(hit) = JSONB_PATH_MEMO.get(&(resource_type.to_string(), expression.to_string())) {
        return hit.clone();
    }
    let paths = octofhir_fhirpath::parse_expression(expression)
        .ok()
        .and_then(|ast| ast_to_jsonb_segments(&ast, resource_type))
        .unwrap_or_default();
    JSONB_PATH_MEMO.insert(
        (resource_type.to_string(), expression.to_string()),
        paths.clone(),
    );
    paths
}

/// Like [`fhirpath_to_jsonb_path`] but returns EVERY same-resource branch of a
/// union expression, not just the first. `combo-*` search params bind to a union
/// of co-located paths within one resource — e.g. combo-value-quantity is
/// `Observation.value.ofType(Quantity) | Observation.component.value.ofType(Quantity)`
/// — and must be searched as an OR across both the top-level and component paths.
/// Cross-resource unions (the shared clinical params) still collapse to the one
/// branch whose leading resource type matches `rt`. Returns one path for a
/// non-union expression; empty when nothing resolves.
pub fn fhirpath_to_jsonb_paths(expression: &str, resource_type: &str) -> Vec<Vec<String>> {
    let Ok(ast) = octofhir_fhirpath::parse_expression(expression) else {
        return Vec::new();
    };
    let mut branches = Vec::new();
    flatten_union(&ast, &mut branches);
    let mut out: Vec<Vec<String>> = Vec::new();
    for b in branches {
        // Keep branches scoped to this resource type (or with no resource prefix);
        // drop other resources' branches in a cross-resource union.
        let leading = ast_leading_identifier(b);
        if leading.is_some() && leading.as_deref() != Some(resource_type) {
            continue;
        }
        if let Some(segs) = ast_to_jsonb_segments(b, resource_type)
            && !segs.is_empty()
            && !out.contains(&segs)
        {
            out.push(segs);
        }
    }
    out
}

/// JSONB property paths to extract for a search parameter's functional index AND
/// the matching query predicate (both must derive paths identically so the planner
/// uses the index). Enum-driven, no string heuristics:
/// - HumanName -> its searchable text subfields (family/given/prefix/suffix/text)
/// - Period    -> start/end (date bounds)
/// - everything else -> the path as-is
pub fn extraction_paths(segments: &[String], hint: &ElementTypeHint) -> Vec<Vec<String>> {
    let with_leaf = |leaf: &str| {
        let mut p = segments.to_vec();
        p.push(leaf.to_string());
        p
    };
    if hint.is_human_name() {
        ["family", "given", "prefix", "suffix", "text"]
            .iter()
            .map(|l| with_leaf(l))
            .collect()
    } else if hint.is_period() {
        ["start", "end"].iter().map(|l| with_leaf(l)).collect()
    } else {
        vec![segments.to_vec()]
    }
}

/// JSONB property paths for a DATE search parameter, split by interval bound.
///
/// A FHIR date search parameter binds to `date | dateTime | instant | Period |
/// Timing`, but the parameter expression is untyped (e.g. `Observation.effective`)
/// while the resource stores the value under the concrete polymorphic key
/// (`effectiveDateTime`, `effectivePeriod`, `effectiveTiming`, …). The base
/// segment may also be a plain scalar (`birthDate`) or a concrete `Period`
/// (`CarePlan.period`). Rather than depend on schema type resolution (which is
/// per-branch and lossy for choices), emit every shape the date type system
/// allows; non-existent keys extract nothing, and `fhir_date_bound` returns NULL
/// for any non-date text (e.g. a Period object at the bare base path), so extra
/// paths are harmless.
///
/// The interval LOWER bound comes from the *lower* paths (`fhir_extract_date_min`),
/// the UPPER bound from the *upper* paths (`fhir_extract_date_max`). They differ
/// only on `Period`: the lower set reads `period.start`, the upper set reads
/// `period.end`. An open Period (`{end}` with no `start`, or `{start}` with no
/// `end`) therefore yields NULL on the missing side — i.e. `(-∞, end]` or
/// `[start, +∞)` — which is the correct FHIR half-bounded semantics. Both the
/// index and the predicate derive paths identically so the planner still matches
/// the functional GiST index.
pub fn date_lower_paths(segments: &[String]) -> Vec<Vec<String>> {
    date_bound_paths(segments, "start")
}

/// Upper-bound extraction paths for a date parameter — see [`date_lower_paths`].
pub fn date_upper_paths(segments: &[String]) -> Vec<Vec<String>> {
    date_bound_paths(segments, "end")
}

fn date_bound_paths(segments: &[String], period_bound: &str) -> Vec<Vec<String>> {
    let Some((leaf, parent)) = segments.split_last() else {
        return vec![Vec::new()];
    };
    let poly = |suffix: &str, extra: &[&str]| {
        let mut p = parent.to_vec();
        p.push(format!("{leaf}{suffix}"));
        p.extend(extra.iter().map(|s| s.to_string()));
        p
    };
    let sub = |extra: &str| {
        let mut p = segments.to_vec();
        p.push(extra.to_string());
        p
    };
    vec![
        // Scalar date/dateTime/instant — base as-is and polymorphic variants
        // (each a point that bounds both sides via fhir_date_bound precision).
        segments.to_vec(),
        poly("DateTime", &[]),
        poly("Date", &[]),
        poly("Instant", &[]),
        // Period — this bound only: concrete (`period.start`) and choice
        // (`effectivePeriod.start`). Absent on an open Period -> NULL -> ±∞.
        sub(period_bound),
        poly("Period", &[period_bound]),
        // Timing.event — points, bound both sides.
        sub("event"),
        poly("Timing", &["event"]),
    ]
}

/// Scalar (point-valued) date extraction paths for the per-occurrence multirange
/// index — every shape whose value is a single date/dateTime/instant string.
/// Period objects are handled separately by [`date_period_object_paths`] so each
/// occurrence's start/end stay paired. See [`fhir_extract_date_multirange`] (SQL).
pub fn date_scalar_paths(segments: &[String]) -> Vec<Vec<String>> {
    let Some((leaf, parent)) = segments.split_last() else {
        return vec![Vec::new()];
    };
    let poly = |suffix: &str, extra: &[&str]| {
        let mut p = parent.to_vec();
        p.push(format!("{leaf}{suffix}"));
        p.extend(extra.iter().map(|s| s.to_string()));
        p
    };
    let sub = |extra: &str| {
        let mut p = segments.to_vec();
        p.push(extra.to_string());
        p
    };
    vec![
        segments.to_vec(),
        poly("DateTime", &[]),
        poly("Date", &[]),
        poly("Instant", &[]),
        // Timing.event — point dates.
        sub("event"),
        poly("Timing", &["event"]),
    ]
}

/// Period-OBJECT extraction paths for the multirange index — the paths point at
/// the Period object itself (not `.start`/`.end`), so the SQL reads `start` and
/// `end` from the SAME object and keeps each occurrence's interval intact. The
/// bare base segment is included because a date param may bind a concrete Period
/// (e.g. `CarePlan.period`); for a scalar base it extracts no object and is a
/// harmless no-op.
pub fn date_period_object_paths(segments: &[String]) -> Vec<Vec<String>> {
    let Some((leaf, parent)) = segments.split_last() else {
        return vec![Vec::new()];
    };
    let poly_period = {
        let mut p = parent.to_vec();
        p.push(format!("{leaf}Period"));
        p
    };
    vec![segments.to_vec(), poly_period]
}

/// Serialize extraction paths to the JSONB literal the `fhir_extract_*` functions
/// accept, e.g. `[["name","family"],["name","given"]]`.
pub fn paths_to_json(paths: &[Vec<String>]) -> String {
    serde_json::to_string(paths).unwrap_or_else(|_| "[]".to_string())
}

/// Serialize extraction paths to a SQL `jsonpath[]` array literal of PRECOMPILED
/// jsonpath constants for the `fhir_extract_*(resource, jsonpath[])` overloads,
/// e.g. `ARRAY['$."name"."family"[*]'::jsonpath, '$."name"."given"[*]'::jsonpath]`.
///
/// Each path becomes a lax jsonpath `$."seg1"."seg2"[*]` — identical semantics to
/// the per-row text builder in the jsonb-array `fhir_extract_*` functions (trailing
/// `[*]` lax-unwraps), but compiled ONCE at index/predicate parse time instead of
/// per row. The index DDL and the search predicate MUST both build their date
/// expressions through this helper so the functional GiST index still matches.
/// jsonpath to every quantity `.value` scalar under a (possibly repeating) path:
/// `["component","valueQuantity"]` -> `$."component"[*]."valueQuantity"."value"`.
/// `[*]` is appended after each non-leaf segment so array parents are lax-iterated.
/// Used by BOTH the quantity-array hull predicate (render) and its functional btree
/// index, so the two expressions match textually and the planner uses the index.
pub fn quantity_hull_value_jsonpath(segments: &[String]) -> String {
    let q = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
    let mut s = String::from("$");
    for (i, seg) in segments.iter().enumerate() {
        s.push_str(&format!(".\"{}\"", q(seg)));
        if i + 1 < segments.len() {
            s.push_str("[*]");
        }
    }
    s.push_str(".\"value\"");
    s
}

/// Whole-resource code containment SQL for a composite token component, OR'd over
/// every location (top-level and array-wrapped `component`), as INLINE literals so a
/// partial composite index `WHERE <this>` is provably usable. Used by BOTH the
/// composite render and the partial-index DDL so the two expressions match textually.
/// `code`/`system` come from the parsed token value (`system|code`, system optional).
pub fn composite_token_containment_sql(
    col: &str,
    token_paths: &[Vec<String>],
    system: Option<&str>,
    code: &str,
) -> String {
    let coding = match system {
        Some(s) => serde_json::json!({ "coding": [{ "system": s, "code": code }] }),
        None => serde_json::json!({ "coding": [{ "code": code }] }),
    };
    let arms: Vec<String> = token_paths
        .iter()
        .map(|p| {
            // Nest the coding leaf up the path; the `component` array parent is wrapped
            // in `[...]` so the containment matches an array element.
            let mut acc = coding.clone();
            for seg in p.iter().rev() {
                acc = if seg == "component" {
                    serde_json::json!({ seg.as_str(): [acc] })
                } else {
                    serde_json::json!({ seg.as_str(): acc })
                };
            }
            format!("{col} @> '{}'::jsonb", acc.to_string().replace('\'', "''"))
        })
        .collect();
    if arms.len() == 1 {
        arms.into_iter().next().unwrap()
    } else {
        format!("({})", arms.join(" OR "))
    }
}

/// SQL `jsonpath[]` literal of every quantity `.value` location for `paths`, e.g.
/// `ARRAY['$."valueQuantity"."value"'::jsonpath, '$."component"[*]."valueQuantity"."value"'::jsonpath]`.
/// Folds the top-level AND component locations of a (combo) quantity param into ONE
/// array so a single `fhir_qty_extract_min/max_numeric(resource, <array>)` btree serves
/// every location. Used by BOTH that union min/max index and the search predicate, so
/// the two expressions match textually and the planner uses the index.
pub fn quantity_value_jsonpath_array(paths: &[Vec<String>]) -> String {
    if paths.is_empty() {
        return "ARRAY[]::jsonpath[]".to_string();
    }
    let items = paths
        .iter()
        .map(|segs| {
            let jp = quantity_hull_value_jsonpath(segs).replace('\'', "''");
            format!("'{jp}'::jsonpath")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("ARRAY[{items}]::jsonpath[]")
}

pub fn paths_to_jsonpath_array(paths: &[Vec<String>]) -> String {
    if paths.is_empty() {
        return "ARRAY[]::jsonpath[]".to_string();
    }
    let items = paths
        .iter()
        .map(|segments| {
            let mut jp = String::from("$");
            for seg in segments {
                // jsonpath member accessor with a double-quoted key; escape `\` and `"`.
                let escaped = seg.replace('\\', "\\\\").replace('"', "\\\"");
                jp.push_str(&format!(".\"{escaped}\""));
            }
            jp.push_str("[*]");
            // Embed as a SQL single-quoted string literal (escape single quotes).
            let sql_literal = jp.replace('\'', "''");
            format!("'{sql_literal}'::jsonpath")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("ARRAY[{items}]")
}

/// An extraction path annotated with each segment's array cardinality:
/// `(segment_name, is_array)`. Produced by resolving each path prefix against the
/// element-type resolver at bootstrap so the generated SQL unwraps the right levels.
pub type AnnotatedPath = Vec<(String, bool)>;

/// Deterministic, identifier-safe name for a per-param typed extraction function:
/// `fhir_s_{table}_{code}` with non-alphanumeric chars folded to `_`, lowercased,
/// truncated to PostgreSQL's 63-byte identifier limit.
pub fn typed_extract_fn_name(resource_type: &str, code: &str) -> String {
    let sanitize = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    let table = sanitize(resource_type);
    let safe_code = sanitize(code);
    let mut name = format!("fhir_s_{table}_{safe_code}");
    if name.len() > 63 {
        name.truncate(63);
    }
    name
}

/// Build a per-param TYPE-AWARE flat text-extraction SQL function for an "indexed"
/// STRING search parameter. Returns `(fn_name, create_or_replace_ddl, arr_expr_prefix)`
/// or `None` when there are no extraction paths.
///
/// The function returns `text[]` (all leaf string values across the annotated
/// paths), is `IMMUTABLE PARALLEL SAFE STRICT`, and is used identically in both the
/// functional GIN index and the query predicate so the planner matches. Array
/// segments are unwrapped with `jsonb_array_elements(fhir_arr(...))`; scalar
/// non-leaf segments traverse with `->`; leaves emit text via `#>> '{}'` (unwrapped
/// array element) or `->>'leaf'` (scalar parent).
pub fn build_typed_extract_fn(
    resource_type: &str,
    code: &str,
    annotated_paths: &[AnnotatedPath],
) -> Option<(String, String, String)> {
    if annotated_paths.is_empty() || annotated_paths.iter().all(|p| p.is_empty()) {
        return None;
    }

    let fn_name = typed_extract_fn_name(resource_type, code);

    let mut branches: Vec<String> = Vec::new();
    for path in annotated_paths {
        if path.is_empty() {
            continue;
        }
        branches.push(build_extract_branch(path));
    }
    if branches.is_empty() {
        return None;
    }

    let body = branches.join("\n    UNION ALL\n    ");
    let ddl = format!(
        "CREATE OR REPLACE FUNCTION {fn_name}(resource jsonb)\n\
         RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT AS $$\n  \
         SELECT nullif(array_agg(leaf), '{{}}') FROM (\n    {body}\n  ) t(leaf) WHERE leaf IS NOT NULL\n$$;"
    );

    Some((fn_name.clone(), ddl, fn_name))
}

/// Codegen one `SELECT ... [FROM ...]` branch for a single annotated extraction path.
fn build_extract_branch(path: &AnnotatedPath) -> String {
    let mut current_expr = "resource".to_string();
    let mut froms: Vec<String> = Vec::new();
    let mut alias_idx = 0usize;
    let last = path.len() - 1;

    let mut leaf_expr = String::new();
    for (i, (seg, is_array)) in path.iter().enumerate() {
        let is_leaf = i == last;
        if *is_array {
            // Unwrap this array level to a row source.
            let alias = format!("e{alias_idx}");
            froms.push(format!(
                "jsonb_array_elements(public.fhir_arr({current_expr}->'{seg}')) AS {alias}(value)"
            ));
            current_expr = format!("{alias}.value");
            alias_idx += 1;
            if is_leaf {
                // Leaf already unwrapped to a row value; render it as text.
                leaf_expr = format!("{current_expr} #>> '{{}}'");
            }
        } else if is_leaf {
            // Scalar leaf: read text directly off the parent jsonb.
            leaf_expr = format!("{current_expr}->>'{seg}'");
        } else {
            // Scalar non-leaf: keep traversing as jsonb.
            current_expr = format!("{current_expr}->'{seg}'");
        }
    }

    if froms.is_empty() {
        format!("SELECT {leaf_expr}")
    } else {
        format!("SELECT {leaf_expr} FROM {}", froms.join(", "))
    }
}

/// Type name carried by an `ofType(...)` / `as` argument (`Identifier` or `TypeInfo`).
fn ast_type_name(node: &octofhir_fhirpath::ExpressionNode) -> Option<String> {
    use octofhir_fhirpath::ExpressionNode as E;
    match node {
        E::Identifier(id) => Some(id.name.clone()),
        E::TypeInfo(t) => Some(t.name.clone()),
        _ => None,
    }
}

/// Fold a polymorphic type onto the last path segment: `value` + `Quantity` -> `valueQuantity`.
fn fold_poly_last(segments: &mut [String], ty: &str) {
    if let Some(last) = segments.last_mut() {
        last.push_str(&capitalize_first(ty));
    }
}

/// Walk a parsed FHIRPath AST into JSONB property segments (resource root dropped).
/// Returns None for shapes that don't reduce to a static property path.
fn ast_to_jsonb_segments(
    node: &octofhir_fhirpath::ExpressionNode,
    rt: &str,
) -> Option<Vec<String>> {
    use octofhir_fhirpath::ExpressionNode as E;
    match node {
        // Leading identifier: a resource-type root (uppercase) is dropped; a bare
        // lowercase identifier is a property (expression without resource prefix).
        E::Identifier(id) => {
            if id
                .name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                Some(Vec::new())
            } else {
                Some(vec![id.name.clone()])
            }
        }
        E::PropertyAccess(p) => {
            let mut s = ast_to_jsonb_segments(&p.object, rt)?;
            s.push(p.property.clone());
            Some(s)
        }
        E::Path(p) => {
            let mut s = ast_to_jsonb_segments(&p.base, rt)?;
            s.extend(
                p.path
                    .split('.')
                    .filter(|x| !x.is_empty())
                    .map(str::to_string),
            );
            Some(s)
        }
        E::Parenthesized(inner) => ast_to_jsonb_segments(inner, rt),
        // Drop the index — JSONB predicates match across array elements.
        E::IndexAccess(ix) => ast_to_jsonb_segments(&ix.object, rt),
        // `value as Quantity` -> fold polymorphic key onto `value`.
        E::TypeCast(tc) => {
            let mut s = ast_to_jsonb_segments(&tc.expression, rt)?;
            fold_poly_last(&mut s, &tc.target_type);
            Some(s)
        }
        // `where(...)` — navigation-neutral, keep the filtered base path.
        E::Filter(f) => ast_to_jsonb_segments(&f.base, rt),
        E::MethodCall(m) => {
            let mut s = ast_to_jsonb_segments(&m.object, rt)?;
            if matches!(m.method.as_str(), "ofType" | "as")
                && let Some(ty) = m.arguments.first().and_then(ast_type_name)
            {
                fold_poly_last(&mut s, &ty);
            }
            // where/resolve/first/last/exists/single/tail/... don't change the path.
            Some(s)
        }
        // Union of `ResourceType.path` branches (the shared common params, e.g.
        // clinical `date`: `AllergyIntolerance.recordedDate | … | Observation.effective | …`).
        // Each branch has a DIFFERENT leaf, so the branch whose leading resource
        // type matches `rt` must win — not the left-most one. Falls back to the
        // first resolvable branch (covers `value[x]` unions whose branches all
        // share `rt`, and resource-prefix-less expressions).
        E::Union(_) => {
            let mut branches = Vec::new();
            flatten_union(node, &mut branches);
            if let Some(b) = branches
                .iter()
                .find(|b| ast_leading_identifier(b).as_deref() == Some(rt))
                && let Some(s) = ast_to_jsonb_segments(b, rt)
            {
                return Some(s);
            }
            branches
                .iter()
                .find_map(|b| ast_to_jsonb_segments(b, rt).filter(|s| !s.is_empty()))
        }
        _ => None,
    }
}

/// Collect the leaf branches of a (possibly nested) FHIRPath union into `out`.
fn flatten_union<'a>(
    node: &'a octofhir_fhirpath::ExpressionNode,
    out: &mut Vec<&'a octofhir_fhirpath::ExpressionNode>,
) {
    use octofhir_fhirpath::ExpressionNode as E;
    if let E::Union(u) = node {
        flatten_union(&u.left, out);
        flatten_union(&u.right, out);
    } else {
        out.push(node);
    }
}

/// Leading (left-most) identifier of a branch — its resource-type root, used to
/// match a union branch to the queried resource type.
fn ast_leading_identifier(node: &octofhir_fhirpath::ExpressionNode) -> Option<String> {
    use octofhir_fhirpath::ExpressionNode as E;
    match node {
        E::Identifier(id) => Some(id.name.clone()),
        E::PropertyAccess(p) => ast_leading_identifier(&p.object),
        E::Path(p) => ast_leading_identifier(&p.base),
        E::Parenthesized(inner) => ast_leading_identifier(inner),
        E::IndexAccess(ix) => ast_leading_identifier(&ix.object),
        E::TypeCast(tc) => ast_leading_identifier(&tc.expression),
        E::Filter(f) => ast_leading_identifier(&f.base),
        E::MethodCall(m) => ast_leading_identifier(&m.object),
        E::Union(u) => ast_leading_identifier(&u.left),
        _ => None,
    }
}

/// Capitalize the first ASCII letter (FHIR polymorphic key casing: `quantity`->`Quantity`,
/// `dateTime`->`DateTime`).
fn capitalize_first(s: &str) -> String {
    let s = s.trim();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Build a JSONB accessor chain from path segments.
///
/// For example: `["name", "family"]` becomes `resource->'name'->'family'`
/// SQL boolean fragment, true when the resource at `resource_col` carries a
/// reference at `ref_segments` whose `reference` string satisfies `ref_predicate`.
///
/// `ref_predicate` is evaluated against each reference element aliased `ref`
/// (e.g. `ref->>'reference' = 'Patient/' || c.id`). Searches the resource JSONB
/// in place — no sidecar index tables.
pub fn jsonb_reference_match_exists_expr(
    resource_col: &str,
    ref_segments: &[String],
    ref_predicate: &str,
) -> crate::ir::sql::SqlExpr {
    use crate::ir::sql::{SelectStmt, SqlExpr, SqlFrom, SqlTerm};
    let obj_path = build_jsonb_accessor(resource_col, ref_segments, false);
    let arr = format!(
        "CASE WHEN jsonb_typeof({obj_path}) = 'array' THEN {obj_path} \
         WHEN {obj_path} IS NULL THEN '[]'::jsonb ELSE jsonb_build_array({obj_path}) END"
    );
    SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: format!("jsonb_array_elements({arr})"),
            alias: Some("ref".to_string()),
        },
        where_clause: Some(SqlExpr::Raw(ref_predicate.to_string())),
    }))
}

pub fn build_jsonb_accessor(resource_col: &str, path: &[String], as_text: bool) -> String {
    if path.is_empty() {
        return resource_col.to_string();
    }

    let mut result = resource_col.to_string();

    for (i, segment) in path.iter().enumerate() {
        if i == path.len() - 1 && as_text {
            // Last segment: use ->> for text extraction
            result = format!("{result}->>'{segment}'");
        } else {
            // Intermediate segments: use -> for JSON traversal
            result = format!("{result}->'{segment}'");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fhirpath_to_jsonb_path() {
        let path = fhirpath_to_jsonb_path("Patient.name.family", "Patient");
        assert_eq!(path, vec!["name", "family"]);

        let path = fhirpath_to_jsonb_path("Resource.id", "Patient");
        assert_eq!(path, vec!["id"]);

        let path = fhirpath_to_jsonb_path("Patient.identifier", "Patient");
        assert_eq!(path, vec!["identifier"]);
    }

    #[test]
    fn test_fhirpath_to_jsonb_path_union() {
        // Union expressions with matching resource type
        let path = fhirpath_to_jsonb_path(
            "Patient.birthDate | Person.birthDate | RelatedPerson.birthDate",
            "Patient",
        );
        assert_eq!(path, vec!["birthDate"]);

        // Union expressions with different resource type - should use matching one
        let path = fhirpath_to_jsonb_path(
            "Patient.birthDate | Person.birthDate | RelatedPerson.birthDate",
            "Person",
        );
        assert_eq!(path, vec!["birthDate"]);

        // Union expressions where we fall back to first when no match
        let path = fhirpath_to_jsonb_path(
            "Patient.name | Practitioner.name | Organization.name",
            "Unknown",
        );
        assert_eq!(path, vec!["name"]);
    }

    #[test]
    fn test_fhirpath_clinical_date_union_picks_resource_branch() {
        // The shared clinical `date` SearchParameter unions many resource types,
        // each with a DIFFERENT leaf. The branch for the queried resource type
        // must win — not the left-most one.
        let expr = "AllergyIntolerance.recordedDate | CarePlan.period | \
                    ClinicalImpression.date | Composition.date | Consent.dateTime | \
                    DiagnosticReport.effective | Encounter.period | \
                    Immunization.occurrence | List.date | Observation.effective | \
                    Procedure.performed | (RiskAssessment.occurrence as dateTime) | \
                    SupplyRequest.authoredOn";
        assert_eq!(
            fhirpath_to_jsonb_path(expr, "Observation"),
            vec!["effective"]
        );
        assert_eq!(
            fhirpath_to_jsonb_path(expr, "AllergyIntolerance"),
            vec!["recordedDate"]
        );
        assert_eq!(fhirpath_to_jsonb_path(expr, "CarePlan"), vec!["period"]);
    }

    #[test]
    fn test_fhirpath_to_jsonb_path_where_resolve() {
        // subject.where(resolve() is Patient) → just "subject"
        let path = fhirpath_to_jsonb_path(
            "Observation.subject.where(resolve() is Patient)",
            "Observation",
        );
        assert_eq!(path, vec!["subject"]);

        // More complex: performer.where(resolve() is Practitioner)
        let path = fhirpath_to_jsonb_path(
            "Encounter.participant.individual.where(resolve() is Practitioner)",
            "Encounter",
        );
        assert_eq!(path, vec!["participant", "individual"]);

        // Simple resolve() without where
        let path = fhirpath_to_jsonb_path("Observation.subject.resolve()", "Observation");
        assert_eq!(path, vec!["subject"]);
    }

    #[test]
    fn test_fhirpath_to_jsonb_path_as_cast() {
        // `as Type` casting folds the polymorphic type onto the leaf segment:
        // `value as CodeableConcept` -> `valueCodeableConcept`.
        let path = fhirpath_to_jsonb_path(
            "(ActivityDefinition.useContext.value as CodeableConcept)",
            "ActivityDefinition",
        );
        assert_eq!(path, vec!["useContext", "valueCodeableConcept"]);

        let path = fhirpath_to_jsonb_path("Observation.value as Quantity", "Observation");
        assert_eq!(path, vec!["valueQuantity"]);
    }

    #[test]
    fn test_build_jsonb_accessor() {
        let path = vec!["name".to_string(), "family".to_string()];

        let accessor = build_jsonb_accessor("resource", &path, true);
        assert_eq!(accessor, "resource->'name'->>'family'");

        let accessor = build_jsonb_accessor("resource", &path, false);
        assert_eq!(accessor, "resource->'name'->'family'");

        let accessor = build_jsonb_accessor("resource", &[], true);
        assert_eq!(accessor, "resource");
    }

    #[test]
    fn test_sql_builder_basic() {
        let mut builder = SqlBuilder::new();

        let p1 = builder.add_text_param("John");
        builder.add_raw_condition(format!(
            "LOWER({}->>'name') LIKE LOWER(${})",
            builder.resource_column(),
            p1
        ));

        assert_eq!(builder.param_count(), 1);
        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        assert_eq!(clause.unwrap(), "LOWER(resource->>'name') LIKE LOWER($1)");
    }

    #[test]
    fn test_sql_builder_multiple_conditions() {
        let mut builder = SqlBuilder::new();

        let p1 = builder.add_text_param("John%");
        builder.add_raw_condition(format!("LOWER(resource->>'name') LIKE LOWER(${})", p1));

        let p2 = builder.add_text_param("active");
        builder.add_raw_condition(format!("resource->>'status' = ${}", p2));

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("AND"));
        assert!(clause.contains("$1"));
        assert!(clause.contains("$2"));
    }

    #[test]
    fn test_sql_builder_or_clause() {
        let conditions = vec![
            "resource->>'status' = 'active'".to_string(),
            "resource->>'status' = 'completed'".to_string(),
        ];

        let or_clause = SqlBuilder::build_or_clause(&conditions);
        assert_eq!(
            or_clause,
            "(resource->>'status' = 'active' OR resource->>'status' = 'completed')"
        );
    }

    #[test]
    fn test_sql_builder_empty() {
        let builder = SqlBuilder::new();
        assert!(builder.build_where_clause().is_none());
    }

    // ========================================================================
    // FhirQueryBuilder Tests
    // ========================================================================

    #[test]
    fn test_jsonb_path_valid() {
        let path = JsonbPath::new(vec!["name".into(), "family".into()]).unwrap();
        assert_eq!(path.segments(), &["name", "family"]);
        assert!(!path.is_array_element());
    }

    #[test]
    fn test_jsonb_path_array_element() {
        let path = JsonbPath::array_element(vec!["identifier".into()]).unwrap();
        assert!(path.is_array_element());
    }

    #[test]
    fn test_jsonb_path_invalid_chars() {
        let result = JsonbPath::new(vec!["name; DROP TABLE".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jsonb_path_empty_segment() {
        let result = JsonbPath::new(vec!["".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jsonb_path_to_accessor() {
        let path = JsonbPath::new(vec!["name".into(), "family".into()]).unwrap();
        let accessor = path.to_accessor("resource", true);
        assert_eq!(accessor, "resource->'name'->>'family'");
    }

    #[test]
    fn test_escape_identifier_valid() {
        let result = escape_identifier("patient").unwrap();
        assert_eq!(result, "\"patient\"");
    }

    #[test]
    fn test_escape_identifier_invalid() {
        let result = escape_identifier("patient; DROP");
        assert!(result.is_err());
    }

    #[test]
    fn test_fhir_query_builder_simple() {
        let path = JsonbPath::new(vec!["gender".into()]).unwrap();
        let query = FhirQueryBuilder::new("Patient", "public")
            .where_condition(SearchCondition::simple(
                path,
                Operator::Eq,
                SqlValue::Text("female".into()),
            ))
            .build()
            .unwrap();

        assert!(query.sql.contains("\"public\".\"patient\""));
        assert!(query.sql.contains("WHERE"));
        assert!(query.sql.contains("$1"));
        assert_eq!(query.params.len(), 1);
    }

    #[test]
    fn test_fhir_query_builder_with_pagination() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .paginate(10, 20)
            .build()
            .unwrap();

        assert!(query.sql.contains("LIMIT 10"));
        assert!(query.sql.contains("OFFSET 20"));
    }

    #[test]
    fn test_fhir_query_builder_with_sort() {
        let path = JsonbPath::new(vec!["name".into(), "family".into()]).unwrap();
        let query = FhirQueryBuilder::new("Patient", "public")
            .sort_by(SortSpec::asc(path))
            .build()
            .unwrap();

        assert!(query.sql.contains("ORDER BY"));
        assert!(query.sql.contains("ASC"));
        assert!(query.sql.contains("NULLS LAST"));
    }

    #[test]
    fn test_fhir_query_builder_with_column_sort() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .sort_by(SortSpec::column("updated_at", SortOrder::Desc).unwrap())
            .build()
            .unwrap();

        assert!(query.sql.contains("ORDER BY \"r\".\"updated_at\" DESC"));
    }

    #[test]
    fn test_fhir_query_builder_count_mode() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .count_only()
            .build()
            .unwrap();

        assert!(query.sql.contains("COUNT(*)"));
        assert!(!query.sql.contains("LIMIT"));
        assert!(!query.sql.contains("ORDER BY"));
    }

    #[test]
    fn test_fhir_query_builder_or_conditions() {
        let path = JsonbPath::new(vec!["status".into()]).unwrap();
        let conditions = vec![
            SearchCondition::simple(path.clone(), Operator::Eq, SqlValue::Text("active".into())),
            SearchCondition::simple(path, Operator::Eq, SqlValue::Text("completed".into())),
        ];
        let query = FhirQueryBuilder::new("Observation", "public")
            .where_condition(SearchCondition::or(conditions))
            .build()
            .unwrap();

        assert!(query.sql.contains(" OR "));
        assert_eq!(query.params.len(), 2);
    }

    #[test]
    fn test_fhir_query_builder_and_conditions() {
        let path1 = JsonbPath::new(vec!["status".into()]).unwrap();
        let path2 = JsonbPath::new(vec!["code".into()]).unwrap();
        let query = FhirQueryBuilder::new("Observation", "public")
            .where_condition(SearchCondition::simple(
                path1,
                Operator::Eq,
                SqlValue::Text("final".into()),
            ))
            .where_condition(SearchCondition::simple(
                path2,
                Operator::Eq,
                SqlValue::Text("12345".into()),
            ))
            .build()
            .unwrap();

        assert!(query.sql.contains(" AND "));
        assert_eq!(query.params.len(), 2);
    }

    #[test]
    fn test_fhir_query_builder_not_condition() {
        let path = JsonbPath::new(vec!["status".into()]).unwrap();
        let condition = SearchCondition::negate(SearchCondition::simple(
            path,
            Operator::Eq,
            SqlValue::Text("cancelled".into()),
        ));
        let query = FhirQueryBuilder::new("Observation", "public")
            .where_condition(condition)
            .build()
            .unwrap();

        assert!(query.sql.contains("NOT"));
    }

    #[test]
    fn test_fhir_query_builder_exists_condition() {
        let path = JsonbPath::new(vec!["deceased".into()]).unwrap();
        let query = FhirQueryBuilder::new("Patient", "public")
            .where_condition(SearchCondition::Exists { path, exists: true })
            .build()
            .unwrap();

        assert!(query.sql.contains("IS NOT NULL"));
    }

    #[test]
    fn test_fhir_query_builder_chain_join() {
        let ref_path = JsonbPath::new(vec!["subject".into(), "reference".into()]).unwrap();
        let _name_path = JsonbPath::new(vec!["name".into(), "family".into()]).unwrap();

        let query = FhirQueryBuilder::new("Observation", "public")
            .chain_join(ChainJoin::new("Observation", "Patient", ref_path, "p"))
            .where_condition(SearchCondition::Raw {
                sql: "\"p\".resource->'name'->>'family' ILIKE $1".into(),
                params: vec![SqlValue::Text("%smith%".into())],
            })
            .build()
            .unwrap();

        assert!(query.sql.contains("INNER JOIN"));
        assert!(query.sql.contains("\"patient\""));
    }

    #[test]
    fn test_fhir_query_builder_explain() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .build_explain()
            .unwrap();

        assert!(query.sql.starts_with("EXPLAIN ANALYZE"));
    }

    #[test]
    fn test_fhir_query_builder_build_count() {
        let path = JsonbPath::new(vec!["status".into()]).unwrap();
        let query = FhirQueryBuilder::new("Patient", "public")
            .where_condition(SearchCondition::simple(
                path,
                Operator::Eq,
                SqlValue::Text("active".into()),
            ))
            .build_count()
            .unwrap();

        assert!(query.sql.contains("COUNT(*)"));
        assert!(query.sql.contains("WHERE"));
    }

    #[test]
    fn test_fhir_query_builder_too_many_conditions() {
        let path = JsonbPath::new(vec!["status".into()]).unwrap();
        let mut builder = FhirQueryBuilder::new("Patient", "public");

        for i in 0..101 {
            builder = builder.where_condition(SearchCondition::simple(
                path.clone(),
                Operator::Eq,
                SqlValue::Text(format!("value{i}")),
            ));
        }

        let result = builder.build();
        assert!(result.is_err());
        assert!(matches!(result, Err(SqlBuilderError::QueryTooComplex(_))));
    }

    #[test]
    fn test_fhir_query_builder_ilike_operator() {
        let path = JsonbPath::new(vec!["name".into(), "family".into()]).unwrap();
        let query = FhirQueryBuilder::new("Patient", "public")
            .where_condition(SearchCondition::simple(
                path,
                Operator::ILike,
                SqlValue::Text("%smith%".into()),
            ))
            .build()
            .unwrap();

        assert!(query.sql.contains("ILIKE"));
    }

    #[test]
    fn test_fhir_query_builder_ids_only_mode() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .ids_only()
            .build()
            .unwrap();

        assert!(query.sql.contains("SELECT r.id"));
        assert!(!query.sql.contains("resource"));
    }

    #[test]
    fn test_fhir_query_builder_raw_condition() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .where_condition(SearchCondition::raw(
                "r.resource->>'birthDate' >= $1",
                vec![SqlValue::Text("1990-01-01".into())],
            ))
            .build()
            .unwrap();

        assert!(query.sql.contains("birthDate"));
        assert_eq!(query.params.len(), 1);
    }

    #[test]
    fn test_fhir_query_builder_with_alias() {
        let query = FhirQueryBuilder::new("Patient", "public")
            .with_alias("pat")
            .build()
            .unwrap();

        assert!(query.sql.contains("AS pat"));
        assert!(query.sql.contains("pat.resource"));
    }

    #[test]
    fn test_sql_value_display() {
        assert_eq!(SqlValue::Text("hello".into()).as_display_str(), "hello");
        assert_eq!(SqlValue::Integer(42).as_display_str(), "42");
        assert_eq!(SqlValue::Float(2.71).as_display_str(), "2.71");
        assert_eq!(SqlValue::Boolean(true).as_display_str(), "true");
        assert_eq!(SqlValue::Null.as_display_str(), "NULL");
    }

    #[test]
    fn test_operator_as_sql() {
        assert_eq!(Operator::Eq.as_sql(), "=");
        assert_eq!(Operator::Ne.as_sql(), "!=");
        assert_eq!(Operator::Gt.as_sql(), ">");
        assert_eq!(Operator::Lt.as_sql(), "<");
        assert_eq!(Operator::Ge.as_sql(), ">=");
        assert_eq!(Operator::Le.as_sql(), "<=");
        assert_eq!(Operator::Like.as_sql(), "LIKE");
        assert_eq!(Operator::ILike.as_sql(), "ILIKE");
        assert_eq!(Operator::Contains.as_sql(), "@>");
        assert_eq!(Operator::IsNull.as_sql(), "IS NULL");
        assert_eq!(Operator::IsNotNull.as_sql(), "IS NOT NULL");
    }

    #[test]
    fn test_search_condition_or_empty() {
        let condition = SearchCondition::or(vec![]);
        assert!(matches!(condition, SearchCondition::False));
    }

    #[test]
    fn test_search_condition_and_empty() {
        let condition = SearchCondition::and(vec![]);
        assert!(matches!(condition, SearchCondition::True));
    }

    #[test]
    fn test_search_condition_or_single() {
        let path = JsonbPath::new(vec!["status".into()]).unwrap();
        let inner = SearchCondition::simple(path, Operator::Eq, SqlValue::Text("active".into()));
        let condition = SearchCondition::or(vec![inner]);

        // Single condition should not be wrapped in Or
        assert!(matches!(condition, SearchCondition::Simple { .. }));
    }

    #[test]
    fn test_built_query_display() {
        let query = BuiltQuery {
            sql: "SELECT * FROM patient".to_string(),
            params: vec![],
        };
        assert_eq!(format!("{}", query), "SELECT * FROM patient");
    }

    // ========================================================================
    // Typed extraction function codegen tests
    // ========================================================================

    /// Collapse all runs of whitespace to a single space and trim, so structural
    /// comparison ignores layout differences (indentation, line breaks).
    fn norm_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn ap(segs: &[(&str, bool)]) -> AnnotatedPath {
        segs.iter().map(|(s, a)| (s.to_string(), *a)).collect()
    }

    #[test]
    fn test_typed_extract_fn_name_basic() {
        assert_eq!(
            typed_extract_fn_name("Patient", "name"),
            "fhir_s_patient_name"
        );
        assert_eq!(
            typed_extract_fn_name("Organization", "name"),
            "fhir_s_organization_name"
        );
    }

    #[test]
    fn test_typed_extract_fn_name_hyphen_to_underscore() {
        assert_eq!(
            typed_extract_fn_name("Patient", "general-practitioner"),
            "fhir_s_patient_general_practitioner"
        );
    }

    #[test]
    fn test_typed_extract_fn_name_truncates_to_63() {
        let long_code = "a".repeat(80);
        let name = typed_extract_fn_name("Patient", &long_code);
        assert!(name.len() <= 63, "name was {} bytes: {name}", name.len());
        assert!(name.starts_with("fhir_s_patient_"));
    }

    #[test]
    fn test_build_typed_extract_fn_empty_paths_none() {
        assert!(build_typed_extract_fn("Patient", "name", &[]).is_none());
        assert!(build_typed_extract_fn("Patient", "name", &[vec![]]).is_none());
    }

    #[test]
    fn test_build_typed_extract_fn_scalar_only() {
        let paths = vec![ap(&[("gender", false)])];
        let (fn_name, ddl, prefix) = build_typed_extract_fn("Patient", "gender", &paths).unwrap();
        assert_eq!(fn_name, "fhir_s_patient_gender");
        assert_eq!(prefix, "fhir_s_patient_gender");
        let n = norm_ws(&ddl);
        assert!(n.contains("CREATE OR REPLACE FUNCTION fhir_s_patient_gender(resource jsonb)"));
        assert!(n.contains("RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT"));
        assert!(n.contains("SELECT nullif(array_agg(leaf), '{}') FROM ("));
        assert!(n.contains("SELECT resource->>'gender'"));
        assert!(n.contains(") t(leaf) WHERE leaf IS NOT NULL"));
        assert!(!n.contains("UNION ALL"));
    }

    #[test]
    fn test_build_typed_extract_fn_single_array() {
        // alias is an array of strings: Organization.alias
        let paths = vec![ap(&[("alias", true)])];
        let (_, ddl, _) = build_typed_extract_fn("Organization", "alias", &paths).unwrap();
        let n = norm_ws(&ddl);
        assert!(n.contains(
            "SELECT e0.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'alias')) AS e0(value)"
        ));
    }

    #[test]
    fn test_build_typed_extract_fn_nested_array() {
        // Patient.name.given : name is array, given is array
        let paths = vec![ap(&[("name", true), ("given", true)])];
        let (_, ddl, _) = build_typed_extract_fn("Patient", "given", &paths).unwrap();
        let n = norm_ws(&ddl);
        assert!(n.contains(
            "SELECT e1.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value), jsonb_array_elements(public.fhir_arr(e0.value->'given')) AS e1(value)"
        ));
    }

    #[test]
    fn test_build_typed_extract_fn_multi_path_golden_organization_name() {
        // Organization.name: scalar name + array alias
        let paths = vec![ap(&[("name", false)]), ap(&[("alias", true)])];
        let (fn_name, ddl, _) = build_typed_extract_fn("Organization", "name", &paths).unwrap();
        assert_eq!(fn_name, "fhir_s_organization_name");
        let n = norm_ws(&ddl);
        let expected = norm_ws(
            "CREATE OR REPLACE FUNCTION fhir_s_organization_name(resource jsonb)
            RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT AS $$
              SELECT nullif(array_agg(leaf), '{}') FROM (
                SELECT resource->>'name'
                UNION ALL
                SELECT e0.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'alias')) AS e0(value)
              ) t(leaf) WHERE leaf IS NOT NULL
            $$;",
        );
        assert_eq!(n, expected, "\n  got: {n}\n want: {expected}");
    }

    #[test]
    fn test_build_typed_extract_fn_golden_patient_name() {
        // Patient.name (HumanName): name is array; family/text scalar leaves,
        // given/prefix/suffix array leaves.
        let paths = vec![
            ap(&[("name", true), ("family", false)]),
            ap(&[("name", true), ("given", true)]),
            ap(&[("name", true), ("prefix", true)]),
            ap(&[("name", true), ("suffix", true)]),
            ap(&[("name", true), ("text", false)]),
        ];
        let (fn_name, ddl, _) = build_typed_extract_fn("Patient", "name", &paths).unwrap();
        assert_eq!(fn_name, "fhir_s_patient_name");
        let n = norm_ws(&ddl);
        let expected = norm_ws(
            "CREATE OR REPLACE FUNCTION fhir_s_patient_name(resource jsonb)
            RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT AS $$
              SELECT nullif(array_agg(leaf), '{}') FROM (
                SELECT e0.value->>'family' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value)
                UNION ALL
                SELECT e1.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value), jsonb_array_elements(public.fhir_arr(e0.value->'given')) AS e1(value)
                UNION ALL
                SELECT e1.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value), jsonb_array_elements(public.fhir_arr(e0.value->'prefix')) AS e1(value)
                UNION ALL
                SELECT e1.value #>> '{}' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value), jsonb_array_elements(public.fhir_arr(e0.value->'suffix')) AS e1(value)
                UNION ALL
                SELECT e0.value->>'text' FROM jsonb_array_elements(public.fhir_arr(resource->'name')) AS e0(value)
              ) t(leaf) WHERE leaf IS NOT NULL
            $$;",
        );
        assert_eq!(n, expected, "\n  got: {n}\n want: {expected}");
    }
}
