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
    pub path: JsonbPath,
    pub order: SortOrder,
    pub nulls_last: bool,
}

impl SortSpec {
    pub fn new(path: JsonbPath, order: SortOrder) -> Self {
        Self {
            path,
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
}

impl RevIncludeSpec {
    pub fn new(source_type: impl Into<String>, param_name: impl Into<String>) -> Self {
        Self {
            source_type: source_type.into(),
            param_name: param_name.into(),
            target_type: None,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target_type = Some(target.into());
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
        }
    }

    /// Set a table alias for the main resource table.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.table_alias = Some(alias.into());
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
        let select_clause = match self.mode {
            QueryMode::Count => "SELECT COUNT(*) as total".to_string(),
            QueryMode::IdsOnly => format!("SELECT {alias}.id"),
            QueryMode::Resources | QueryMode::ResourcesWithTotal => {
                format!(
                    "{alias}.resource, {alias}.id, {alias}.txid, {alias}.created_at, {alias}.updated_at"
                )
            }
        };

        // Build FROM clause with JOINs
        let from_clause = self.build_from_clause(&full_table, alias)?;

        // Build WHERE clause
        let where_clause = self.build_where_clause(&resource_col, &mut params)?;

        // Build ORDER BY clause
        let order_clause = self.build_order_clause(&resource_col);

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

                // Replace $1, $2, etc. with actual parameter numbers
                let mut result = sql.clone();
                for i in (1..=p.len()).rev() {
                    result = result.replace(&format!("${i}"), &format!("${}", start_param + i));
                }
                Ok(result)
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

    fn build_order_clause(&self, resource_col: &str) -> String {
        if self.sort.is_empty() {
            return String::new();
        }

        self.sort
            .iter()
            .map(|s| {
                let accessor = s.path.to_accessor(resource_col, true);
                let nulls = if s.nulls_last { " NULLS LAST" } else { "" };
                format!("{accessor} {}{nulls}", s.order.as_sql())
            })
            .collect::<Vec<_>>()
            .join(", ")
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

    /// Build queries for _include specifications.
    pub fn build_include_queries(&self) -> Result<Vec<BuiltQuery>, SqlBuilderError> {
        // For each include, generate a separate query
        let mut queries = Vec::new();

        for include in &self.includes {
            let target_type = include
                .target_type
                .as_deref()
                .unwrap_or(&include.param_name);
            let target_table = target_type.to_lowercase();
            let target_table_escaped = escape_identifier(&target_table)?;
            let schema = escape_identifier(&self.schema)?;

            // This is a simplified include query - real implementation would
            // extract reference IDs from main query results
            let sql = format!(
                "SELECT resource, id, txid, created_at, updated_at FROM {schema}.{target_table_escaped} WHERE id = ANY($1)"
            );

            queries.push(BuiltQuery {
                sql,
                params: vec![],
            });
        }

        Ok(queries)
    }

    /// Build queries for _revinclude specifications.
    pub fn build_revinclude_queries(&self) -> Result<Vec<BuiltQuery>, SqlBuilderError> {
        let mut queries = Vec::new();

        for revinclude in &self.revincludes {
            let source_table = revinclude.source_type.to_lowercase();
            let source_table_escaped = escape_identifier(&source_table)?;
            let schema = escape_identifier(&self.schema)?;

            // Build the reference path for the param
            let ref_path = format!("resource->'{}'->>'reference'", revinclude.param_name);

            // This query finds resources that reference any of the main results
            let sql = format!(
                "SELECT resource, id, txid, created_at, updated_at FROM {schema}.{source_table_escaped} \
                 WHERE ({ref_path}) = ANY($1)"
            );

            queries.push(BuiltQuery {
                sql,
                params: vec![],
            });
        }

        Ok(queries)
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
    conditions: Vec<String>,
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

    /// Add a raw SQL condition with placeholders.
    ///
    /// Use `{}` as placeholder which will be replaced with `$N` parameter references.
    pub fn add_condition(&mut self, condition: impl Into<String>) {
        self.conditions.push(condition.into());
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
    pub fn conditions(&self) -> &[String] {
        &self.conditions
    }

    /// Add a condition for ValueSet expansion result (optimized for large expansions).
    ///
    /// This method handles both small (IN clause) and large (temp table) expansions:
    /// - `InClause`: Generates traditional IN clause with code list
    /// - `TempTable`: Generates JOIN with temp_valueset_codes table
    ///
    /// # Arguments
    ///
    /// * `jsonb_path` - The JSONB path to the coding element (e.g., "resource->'code'")
    /// * `expansion_result` - The expansion result from terminology service
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Small expansion: code IN ('123', '456', '789')
    /// builder.add_valueset_condition("resource->'code'", InClause(concepts));
    ///
    /// // Large expansion: JOIN with temp table
    /// builder.add_valueset_condition("resource->'code'", TempTable(session_id));
    /// ```
    pub fn add_valueset_condition(
        &mut self,
        jsonb_path: &str,
        expansion_result: &crate::terminology::ExpansionResult,
    ) {
        use crate::terminology::ExpansionResult;

        match expansion_result {
            ExpansionResult::InClause(concepts) => {
                // Small expansion: use IN clause with code matching
                if concepts.is_empty() {
                    // Empty ValueSet matches nothing
                    self.add_condition("FALSE");
                    return;
                }

                let mut code_conditions = Vec::new();

                for concept in concepts {
                    let code_param = self.add_text_param(&concept.code);

                    if let Some(ref system) = concept.system {
                        // Match both system and code
                        let system_param = self.add_text_param(system);
                        code_conditions.push(format!(
                            "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
                             WHERE c->>'system' = ${system_param} AND c->>'code' = ${code_param})"
                        ));
                    } else {
                        // Match code only (any system)
                        code_conditions.push(format!(
                            "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
                             WHERE c->>'code' = ${code_param})"
                        ));
                    }
                }

                let condition = if code_conditions.len() == 1 {
                    code_conditions[0].clone()
                } else {
                    format!("({})", code_conditions.join(" OR "))
                };

                self.add_condition(condition);
            }

            ExpansionResult::TempTable(session_id) => {
                // Large expansion: JOIN with temp table for performance
                let session_param = self.add_text_param(session_id);

                let condition = format!(
                    "EXISTS (
                        SELECT 1
                        FROM temp_valueset_codes t
                        CROSS JOIN LATERAL jsonb_array_elements({jsonb_path}->'coding') AS c
                        WHERE t.session_id = ${session_param}
                          AND c->>'code' = t.code
                          AND (t.system = '' OR t.system IS NULL OR c->>'system' = t.system)
                    )"
                );

                self.add_condition(condition);
            }
        }
    }

    /// Build the final WHERE clause by joining conditions with AND.
    ///
    /// Returns `None` if there are no conditions.
    pub fn build_where_clause(&self) -> Option<String> {
        if self.conditions.is_empty() {
            return None;
        }

        let clause = self.conditions.join(" AND ");
        Some(clause)
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
pub fn fhirpath_to_jsonb_path(expression: &str, resource_type: &str) -> Vec<String> {
    // Handle union expressions (|) by finding the matching resource type or using the first one
    let expr = if expression.contains('|') {
        // Split by | and find the one matching our resource type, or use the first
        expression
            .split('|')
            .map(|s| s.trim())
            .find(|s| {
                s.starts_with(&format!("{resource_type}."))
                    || s.starts_with("Resource.")
                    || s.starts_with("DomainResource.")
            })
            .or_else(|| expression.split('|').next().map(|s| s.trim()))
            .unwrap_or(expression)
    } else {
        expression
    };

    // Handle `as Type` casting - extract just the path before 'as'
    let expr = if let Some(idx) = expr.find(" as ") {
        let path_part = &expr[..idx];
        // Remove surrounding parentheses if present
        path_part
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
    } else {
        expr
    };

    // Remove resource type prefix if present
    // Also handle case where we selected a union alternative with a different prefix
    let expr = expr
        .strip_prefix(&format!("{resource_type}."))
        .or_else(|| expr.strip_prefix("Resource."))
        .or_else(|| expr.strip_prefix("DomainResource."))
        .or_else(|| {
            // Try to strip any ResourceType. prefix (for union fallback cases)
            if let Some(idx) = expr.find('.') {
                let potential_type = &expr[..idx];
                // If it looks like a resource type (starts with uppercase), strip it
                if potential_type
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
                {
                    return Some(&expr[idx + 1..]);
                }
            }
            None
        })
        .unwrap_or(expr);

    // Strip FHIRPath function calls that don't map to JSONB paths.
    // These are type filters/resolvers — the type info is already in SearchParameter.target.
    // Examples:
    //   "subject.where(resolve() is Patient)" → "subject"
    //   "value.ofType(Quantity)" → "valueQuantity" (handled separately via polymorphic naming)
    //   "effective.ofType(dateTime)" → "effectiveDateTime"
    let expr = strip_fhirpath_functions(expr);

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

/// Strip FHIRPath function calls from an expression, keeping only property paths.
///
/// FHIRPath expressions like `subject.where(resolve() is Patient)` contain
/// function calls that have no JSONB equivalent — they're type discriminators.
/// The type info is already available in SearchParameter.target, so we only
/// need the underlying property path.
pub(crate) fn strip_fhirpath_functions(expr: &str) -> String {
    let mut result = String::with_capacity(expr.len());
    let mut i = 0;
    let bytes = expr.as_bytes();

    while i < bytes.len() {
        // Look for function calls: identifier followed by '('
        if bytes[i] == b'(' {
            // Found start of function args — skip until matching ')'
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            // Remove the function name we already appended (e.g., "where", "resolve", "ofType")
            // by scanning back to the last '.' or start
            if let Some(dot_pos) = result.rfind('.') {
                let func_candidate = &result[dot_pos + 1..];
                if is_fhirpath_function(func_candidate) {
                    result.truncate(dot_pos);
                }
            } else if is_fhirpath_function(&result) {
                result.clear();
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    // Clean up trailing dots
    while result.ends_with('.') {
        result.pop();
    }
    // Clean up leading dots
    while result.starts_with('.') {
        result.remove(0);
    }

    result
}

/// Check if a string is a known FHIRPath function name.
fn is_fhirpath_function(name: &str) -> bool {
    matches!(
        name,
        "where" | "resolve" | "ofType" | "exists" | "empty" | "first" | "last" | "as" | "is"
            | "not" | "all" | "any" | "count" | "distinct" | "single" | "type"
    )
}

/// Build a JSONB accessor chain from path segments.
///
/// For example: `["name", "family"]` becomes `resource->'name'->'family'`
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
    use crate::terminology::ExpansionResult;
    use octofhir_fhir_model::terminology::ValueSetConcept;

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
        let path = fhirpath_to_jsonb_path(
            "Observation.subject.resolve()",
            "Observation",
        );
        assert_eq!(path, vec!["subject"]);
    }

    #[test]
    fn test_fhirpath_to_jsonb_path_as_cast() {
        // Handle `as Type` casting
        let path = fhirpath_to_jsonb_path(
            "(ActivityDefinition.useContext.value as CodeableConcept)",
            "ActivityDefinition",
        );
        assert_eq!(path, vec!["useContext", "value"]);

        let path = fhirpath_to_jsonb_path("Observation.value as Quantity", "Observation");
        assert_eq!(path, vec!["value"]);
    }

    #[test]
    fn test_add_valueset_condition_inclause_small() {
        let mut builder = SqlBuilder::new();

        let concepts = vec![
            ValueSetConcept {
                code: "123".to_string(),
                system: Some("http://loinc.org".to_string()),
                display: None,
            },
            ValueSetConcept {
                code: "456".to_string(),
                system: Some("http://loinc.org".to_string()),
                display: None,
            },
        ];

        let result = ExpansionResult::InClause(concepts);
        builder.add_valueset_condition("resource->'code'", &result);

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("EXISTS"));
        assert!(clause.contains("jsonb_array_elements"));
        assert!(clause.contains("OR"));
        assert_eq!(builder.params().len(), 4); // 2 systems + 2 codes
    }

    #[test]
    fn test_add_valueset_condition_inclause_empty() {
        let mut builder = SqlBuilder::new();

        let result = ExpansionResult::InClause(vec![]);
        builder.add_valueset_condition("resource->'code'", &result);

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "FALSE");
    }

    #[test]
    fn test_add_valueset_condition_temp_table() {
        let mut builder = SqlBuilder::new();

        let session_id = "test-session-123".to_string();
        let result = ExpansionResult::TempTable(session_id.clone());
        builder.add_valueset_condition("resource->'code'", &result);

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("temp_valueset_codes"));
        assert!(clause.contains("t.session_id"));
        assert!(clause.contains("CROSS JOIN LATERAL"));
        assert_eq!(builder.params().len(), 1); // Just the session_id
        assert_eq!(builder.params()[0].as_str(), session_id);
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
        builder.add_condition(format!(
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
        builder.add_condition(format!("LOWER(resource->>'name') LIKE LOWER(${})", p1));

        let p2 = builder.add_text_param("active");
        builder.add_condition(format!("resource->>'status' = ${}", p2));

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
    fn test_fhir_query_builder_include_spec() {
        let builder = FhirQueryBuilder::new("Observation", "public")
            .include(IncludeSpec::new("Observation", "subject").with_target("Patient"));

        let include_queries = builder.build_include_queries().unwrap();
        assert_eq!(include_queries.len(), 1);
        assert!(include_queries[0].sql.contains("\"patient\""));
    }

    #[test]
    fn test_fhir_query_builder_revinclude_spec() {
        let builder = FhirQueryBuilder::new("Patient", "public")
            .revinclude(RevIncludeSpec::new("Observation", "subject"));

        let revinclude_queries = builder.build_revinclude_queries().unwrap();
        assert_eq!(revinclude_queries.len(), 1);
        assert!(revinclude_queries[0].sql.contains("\"observation\""));
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
}
