//! SQL generation from ViewDefinitions.
//!
//! This module converts ViewDefinition resources into PostgreSQL queries
//! that can be executed against FHIR data stored in JSONB format.

use crate::Error;
use crate::column::ColumnType;
use crate::view_definition::{SelectColumn, ViewDefinition};

/// Represents a parsed FHIRPath segment.
#[derive(Debug, Clone, PartialEq)]
enum PathSegment {
    /// A simple field access (e.g., "name", "family")
    Field(String),
    /// Array index access via first() function
    First,
    /// Array index access via last() function
    Last,
    /// Array index access via numeric index (e.g., [0], [1])
    Index(i32),
    /// The ofType() function with a type parameter
    OfType(String),
    /// The where() function with a condition
    Where(String),
    /// The join() function with a separator
    Join(String),
    /// The exists() function
    Exists,
    /// The extension() function with a URL parameter
    Extension(String),
    /// The getReferenceKey() function with optional resource type
    GetReferenceKey(Option<String>),
    /// The empty() function
    Empty,
    /// The contains() function for string containment
    Contains(String),
    /// The startsWith() function for string prefix matching
    StartsWith(String),
    /// The endsWith() function for string suffix matching
    EndsWith(String),
    /// The count() function for collection length
    Count,
    /// The iif() function for conditional expressions
    Iif(String, String, String),
    /// The distinct() function for removing duplicates
    Distinct,
    /// The not() function for boolean negation
    Not,
    /// The hasValue() function for checking non-empty values
    HasValue,
    /// The matches() function for regex pattern matching
    Matches(String),
}

/// Generates SQL queries from ViewDefinitions.
pub struct SqlGenerator {
    /// The base table name pattern (e.g., "resource" for resource.data).
    table_pattern: String,
}

impl Default for SqlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlGenerator {
    /// Create a new SQL generator with default settings.
    pub fn new() -> Self {
        Self {
            table_pattern: "base".to_string(),
        }
    }

    /// Create a new SQL generator with a custom table pattern.
    pub fn with_table_pattern(table_pattern: impl Into<String>) -> Self {
        Self {
            table_pattern: table_pattern.into(),
        }
    }

    /// Generate SQL from a ViewDefinition.
    ///
    /// # Errors
    ///
    /// Returns an error if the ViewDefinition contains invalid paths or
    /// cannot be converted to SQL.
    pub fn generate(&self, view: &ViewDefinition) -> Result<GeneratedSql, Error> {
        // Build constants map for substitution
        let constants: std::collections::HashMap<String, String> = view
            .constant
            .iter()
            .map(|c| {
                let value = if let Some(s) = &c.value_string {
                    format!("'{}'", s.replace('\'', "''"))
                } else if let Some(i) = c.value_integer {
                    i.to_string()
                } else if let Some(b) = c.value_boolean {
                    b.to_string()
                } else if let Some(d) = c.value_decimal {
                    d.to_string()
                } else {
                    "NULL".to_string()
                };
                (c.name.clone(), value)
            })
            .collect();

        let table = view.resource.to_lowercase();
        let mut columns = Vec::new();
        let mut joins = Vec::new();
        let mut where_clauses = Vec::new();
        let mut ctes = Vec::new();
        let mut join_counter = 0;
        let mut cte_counter = 0;

        // Process select columns
        for select in &view.select {
            self.process_select_with_constants(
                &mut columns,
                &mut joins,
                &mut ctes,
                &mut join_counter,
                &mut cte_counter,
                select,
                &self.table_pattern,
                "",
                &constants,
                &table,
            )?;
        }

        // Process where clauses
        for where_clause in &view.where_ {
            let path = self.substitute_constants(&where_clause.path, &constants);
            let sql = self.fhirpath_to_sql(&path, &self.table_pattern)?;
            where_clauses.push(sql);
        }

        // Build final SQL with type casting
        let column_sql: String = if columns.is_empty() {
            "*".to_string()
        } else {
            columns
                .iter()
                .map(|c| {
                    let cast_expr = self.apply_type_cast(&c.expression, &c.col_type);
                    format!("{} AS \"{}\"", cast_expr, c.alias)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        let mut sql = format!(
            "SELECT {} FROM {} {}",
            column_sql, table, self.table_pattern
        );

        for join in &joins {
            sql.push_str(&format!(" {}", join));
        }

        // Add base where clause for non-deleted resources
        sql.push_str(&format!(
            " WHERE {}.status != 'deleted'",
            self.table_pattern
        ));

        // Add user-defined where clauses
        for clause in &where_clauses {
            sql.push_str(&format!(" AND ({})", clause));
        }

        // Prepend CTEs if any
        if !ctes.is_empty() {
            let cte_sql = ctes.join(", ");
            sql = format!("WITH RECURSIVE {} {}", cte_sql, sql);
        }

        Ok(GeneratedSql { sql, columns, ctes })
    }

    /// Substitute %name constants in a FHIRPath expression.
    fn substitute_constants(
        &self,
        path: &str,
        constants: &std::collections::HashMap<String, String>,
    ) -> String {
        let mut result = path.to_string();
        for (name, value) in constants {
            let pattern = format!("%{}", name);
            result = result.replace(&pattern, value);
        }
        result
    }

    /// Apply SQL type casting based on column type.
    fn apply_type_cast(&self, expression: &str, col_type: &ColumnType) -> String {
        match col_type {
            ColumnType::String => expression.to_string(),
            ColumnType::Integer => format!("({}::bigint)", expression),
            ColumnType::Decimal => format!("({}::numeric)", expression),
            ColumnType::Boolean => format!("({}::boolean)", expression),
            ColumnType::Date => format!("({}::date)", expression),
            ColumnType::DateTime => format!("({}::timestamptz)", expression),
            ColumnType::Instant => format!("({}::timestamptz)", expression),
            ColumnType::Time => format!("({}::time)", expression),
            ColumnType::Base64Binary => format!("decode({}, 'base64')", expression),
            ColumnType::Json => format!("({}::jsonb)", expression),
        }
    }

    /// Process a single select clause with constant substitution.
    #[allow(clippy::too_many_arguments)]
    fn process_select_with_constants(
        &self,
        columns: &mut Vec<GeneratedColumn>,
        joins: &mut Vec<String>,
        ctes: &mut Vec<String>,
        join_counter: &mut usize,
        cte_counter: &mut usize,
        select: &SelectColumn,
        table_alias: &str,
        prefix: &str,
        constants: &std::collections::HashMap<String, String>,
        base_table: &str,
    ) -> Result<(), Error> {
        // Check if this select has repeat expressions
        if !select.repeat.is_empty() {
            return self.process_repeat_select(
                columns,
                ctes,
                cte_counter,
                select,
                table_alias,
                prefix,
                constants,
                base_table,
            );
        }
        // Handle forEach (array expansion)
        if let Some(for_each) = &select.for_each {
            let for_each = self.substitute_constants(for_each, constants);
            let alias = format!("fe_{}", join_counter);
            *join_counter += 1;

            let path_sql = self.fhirpath_to_jsonb_array_path(&for_each)?;

            joins.push(format!(
                "CROSS JOIN LATERAL jsonb_array_elements({}.resource->{}) AS {}(elem)",
                table_alias, path_sql, alias
            ));

            // Process columns with the new context
            if let Some(cols) = &select.column {
                for col in cols {
                    let path = self.substitute_constants(&col.path, constants);
                    let expression = self.fhirpath_to_sql_in_context(&path, &alias, "elem")?;
                    let alias_name = self.make_column_alias(&col.name, prefix);
                    let col_type = col
                        .col_type
                        .as_ref()
                        .map(|t| ColumnType::from_fhir_type(t))
                        .unwrap_or(ColumnType::String);

                    columns.push(GeneratedColumn {
                        name: col.name.clone(),
                        expression,
                        alias: alias_name,
                        col_type,
                    });
                }
            }

            // Process nested selects with new context
            for nested in &select.select {
                self.process_select_with_constants(
                    columns,
                    joins,
                    ctes,
                    join_counter,
                    cte_counter,
                    nested,
                    &alias,
                    prefix,
                    constants,
                    base_table,
                )?;
            }

            return Ok(());
        }

        // Handle forEachOrNull (array expansion with null row for empty arrays)
        if let Some(for_each) = &select.for_each_or_null {
            let for_each = self.substitute_constants(for_each, constants);
            let alias = format!("feon_{}", join_counter);
            *join_counter += 1;

            let path_sql = self.fhirpath_to_jsonb_array_path(&for_each)?;

            joins.push(format!(
                "LEFT JOIN LATERAL jsonb_array_elements({}.resource->{}) AS {}(elem) ON true",
                table_alias, path_sql, alias
            ));

            // Process columns with the new context
            if let Some(cols) = &select.column {
                for col in cols {
                    let path = self.substitute_constants(&col.path, constants);
                    let expression = self.fhirpath_to_sql_in_context(&path, &alias, "elem")?;
                    let alias_name = self.make_column_alias(&col.name, prefix);
                    let col_type = col
                        .col_type
                        .as_ref()
                        .map(|t| ColumnType::from_fhir_type(t))
                        .unwrap_or(ColumnType::String);

                    columns.push(GeneratedColumn {
                        name: col.name.clone(),
                        expression,
                        alias: alias_name,
                        col_type,
                    });
                }
            }

            // Process nested selects
            for nested in &select.select {
                self.process_select_with_constants(
                    columns,
                    joins,
                    ctes,
                    join_counter,
                    cte_counter,
                    nested,
                    &alias,
                    prefix,
                    constants,
                    base_table,
                )?;
            }

            return Ok(());
        }

        // Handle direct columns
        if let Some(cols) = &select.column {
            for col in cols {
                let path = self.substitute_constants(&col.path, constants);
                let expression = self.fhirpath_to_sql(&path, table_alias)?;
                let alias_name = self.make_column_alias(&col.name, prefix);
                let col_type = col
                    .col_type
                    .as_ref()
                    .map(|t| ColumnType::from_fhir_type(t))
                    .unwrap_or(ColumnType::String);

                columns.push(GeneratedColumn {
                    name: col.name.clone(),
                    expression,
                    alias: alias_name,
                    col_type,
                });
            }
        }

        // Handle nested selects
        let new_prefix = if let Some(alias) = &select.alias {
            if prefix.is_empty() {
                alias.clone()
            } else {
                format!("{}_{}", prefix, alias)
            }
        } else {
            prefix.to_string()
        };

        for nested in &select.select {
            self.process_select_with_constants(
                columns,
                joins,
                ctes,
                join_counter,
                cte_counter,
                nested,
                table_alias,
                &new_prefix,
                constants,
                base_table,
            )?;
        }

        // Handle unionAll
        if let Some(union_selects) = &select.union_all {
            for union_select in union_selects {
                self.process_select_with_constants(
                    columns,
                    joins,
                    ctes,
                    join_counter,
                    cte_counter,
                    union_select,
                    table_alias,
                    prefix,
                    constants,
                    base_table,
                )?;
            }
        }

        Ok(())
    }

    /// Process a select clause with repeat expressions using recursive CTEs.
    #[allow(clippy::too_many_arguments)]
    fn process_repeat_select(
        &self,
        columns: &mut Vec<GeneratedColumn>,
        ctes: &mut Vec<String>,
        cte_counter: &mut usize,
        select: &SelectColumn,
        _table_alias: &str,
        prefix: &str,
        constants: &std::collections::HashMap<String, String>,
        base_table: &str,
    ) -> Result<(), Error> {
        // Generate a unique CTE name
        let cte_name = format!("repeat_cte_{}", cte_counter);
        *cte_counter += 1;

        // Get the initial forEach path (base case for recursion)
        let initial_path = select
            .for_each
            .as_ref()
            .or(select.for_each_or_null.as_ref())
            .ok_or_else(|| {
                Error::InvalidPath("repeat requires forEach or forEachOrNull".to_string())
            })?;

        let initial_path = self.substitute_constants(initial_path, constants);
        let path_sql = self.fhirpath_to_jsonb_array_path(&initial_path)?;

        // Build the recursive CTE
        // Base case: initial forEach expansion
        let base_case = format!(
            "SELECT elem.value AS elem, 1 AS depth FROM {} base CROSS JOIN LATERAL jsonb_array_elements(base.resource->{}) AS elem",
            base_table, path_sql
        );

        // Recursive case: traverse repeat paths
        let repeat_paths: Vec<String> = select
            .repeat
            .iter()
            .map(|p| self.substitute_constants(p, constants))
            .collect();

        // For simplicity, use the first repeat path (most common case)
        let repeat_path = repeat_paths
            .first()
            .map(|p| {
                self.fhirpath_to_jsonb_array_path(p)
                    .unwrap_or_else(|_| format!("'{}'", p))
            })
            .unwrap_or_default();

        let recursive_case = format!(
            "SELECT child.value AS elem, parent.depth + 1 FROM {} parent CROSS JOIN LATERAL jsonb_array_elements(parent.elem->{}) AS child WHERE parent.depth < 10",
            cte_name, repeat_path
        );

        let cte_sql = format!(
            "{} AS ({} UNION ALL {})",
            cte_name, base_case, recursive_case
        );
        ctes.push(cte_sql);

        // Process columns using the CTE as the source
        if let Some(cols) = &select.column {
            for col in cols {
                let path = self.substitute_constants(&col.path, constants);
                let expression = self.fhirpath_to_sql_in_context(&path, &cte_name, "elem")?;
                let alias_name = self.make_column_alias(&col.name, prefix);
                let col_type = col
                    .col_type
                    .as_ref()
                    .map(|t| ColumnType::from_fhir_type(t))
                    .unwrap_or(ColumnType::String);

                columns.push(GeneratedColumn {
                    name: col.name.clone(),
                    expression,
                    alias: alias_name,
                    col_type,
                });
            }
        }

        Ok(())
    }

    /// Convert a FHIRPath expression to SQL for the base resource.
    fn fhirpath_to_sql(&self, path: &str, table_alias: &str) -> Result<String, Error> {
        if path.is_empty() {
            return Err(Error::InvalidPath("Empty path".to_string()));
        }

        // Handle special cases
        if path == "id" {
            return Ok(format!("{}.id", table_alias));
        }

        // Handle getResourceKey() function
        if path == "getResourceKey()" {
            return Ok(format!(
                "{}.resource_type || '/' || {}.id",
                table_alias, table_alias
            ));
        }

        // Parse path into typed segments
        let segments = self.parse_fhirpath_segments(path)?;

        if segments.is_empty() {
            return Ok(format!("{}.resource", table_alias));
        }

        self.segments_to_sql(&segments, &format!("{}.resource", table_alias), true)
    }

    /// Convert parsed segments to SQL expression.
    ///
    /// # Arguments
    /// * `segments` - The parsed path segments
    /// * `base_expr` - The base SQL expression to build on (e.g., "base.resource")
    /// * `extract_text` - Whether to extract text (->>') for the final field access
    fn segments_to_sql(
        &self,
        segments: &[PathSegment],
        base_expr: &str,
        extract_text: bool,
    ) -> Result<String, Error> {
        let mut sql = base_expr.to_string();
        let mut pending_join: Option<String> = None;

        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == segments.len() - 1;

            match segment {
                PathSegment::Field(field) => {
                    // Check for field[index] pattern
                    if let Some(bracket_pos) = field.find('[') {
                        let field_name = &field[..bracket_pos];
                        let index_str = &field[bracket_pos + 1..field.len() - 1];
                        if let Ok(index) = index_str.parse::<i32>() {
                            sql = format!("{}->'{}'", sql, field_name);
                            sql = format!("{}->{}", sql, index);
                            continue;
                        }
                    }

                    if is_last && extract_text && pending_join.is_none() {
                        // Last element - use ->> for text extraction
                        sql = format!("{}->>'{}'", sql, field);
                    } else {
                        // Intermediate element or will be joined - use -> for JSON traversal
                        sql = format!("{}->'{}'", sql, field);
                    }
                }
                PathSegment::First => {
                    // first() -> access array element 0
                    sql = format!("{}->0", sql);
                }
                PathSegment::Last => {
                    // last() -> access array element -1 (PostgreSQL supports negative indexing)
                    sql = format!("{}->-1", sql);
                }
                PathSegment::Index(idx) => {
                    // [n] -> access array element n
                    sql = format!("{}->{}", sql, idx);
                }
                PathSegment::Join(separator) => {
                    // join(separator) is applied at the end to the current array
                    pending_join = Some(separator.clone());
                }
                PathSegment::Exists => {
                    // exists() -> check if the value is not null
                    sql = format!("({} IS NOT NULL)", sql);
                }
                PathSegment::OfType(type_name) => {
                    // ofType() filters by FHIR polymorphic type
                    // For now, we handle common value[x] patterns
                    sql = format!("{}->'value{}'", sql, type_name);
                }
                PathSegment::Where(condition) => {
                    // Parse the condition and generate a filtered array subquery
                    let (field, op, value) = self.parse_condition(condition)?;
                    sql = format!(
                        "(SELECT jsonb_agg(elem) FROM jsonb_array_elements({}) AS elem WHERE elem->>'{}' {} '{}')",
                        sql, field, op, value
                    );
                }
                PathSegment::Extension(url) => {
                    // Filter extensions by URL, returning the first matching extension
                    sql = format!(
                        "(SELECT elem FROM jsonb_array_elements({}->'extension') AS elem WHERE elem->>'url' = '{}' LIMIT 1)",
                        sql, url
                    );
                }
                PathSegment::GetReferenceKey(resource_type) => {
                    // Extract the resource ID from a reference using existing fhir_ref_id/fhir_ref_type functions
                    match resource_type {
                        Some(rt) => {
                            // getReferenceKey(Patient) - extract ID only if reference matches type
                            sql = format!(
                                "CASE WHEN fhir_ref_type({}->>'reference') = '{}' THEN fhir_ref_id({}->>'reference') END",
                                sql, rt, sql
                            );
                        }
                        None => {
                            // getReferenceKey() - extract ID from any reference
                            sql = format!("fhir_ref_id({}->>'reference')", sql);
                        }
                    }
                }
                PathSegment::Empty => {
                    // Check if the collection is empty
                    sql = format!(
                        "(CASE WHEN {} IS NULL THEN true WHEN jsonb_typeof({}) = 'array' THEN jsonb_array_length({}) = 0 ELSE false END)",
                        sql, sql, sql
                    );
                }
                PathSegment::Contains(substring) => {
                    // contains(substring) -> LIKE '%substring%'
                    let escaped = substring.replace('\'', "''").replace('%', "\\%");
                    sql = format!("({} LIKE '%{}%')", sql, escaped);
                }
                PathSegment::StartsWith(prefix) => {
                    // startsWith(prefix) -> LIKE 'prefix%'
                    let escaped = prefix.replace('\'', "''").replace('%', "\\%");
                    sql = format!("({} LIKE '{}%')", sql, escaped);
                }
                PathSegment::EndsWith(suffix) => {
                    // endsWith(suffix) -> LIKE '%suffix'
                    let escaped = suffix.replace('\'', "''").replace('%', "\\%");
                    sql = format!("({} LIKE '%{}')", sql, escaped);
                }
                PathSegment::Count => {
                    // count() -> array length or 1 for scalars
                    sql = format!(
                        "(CASE WHEN jsonb_typeof({}) = 'array' THEN jsonb_array_length({}) ELSE CASE WHEN {} IS NOT NULL THEN 1 ELSE 0 END END)",
                        sql, sql, sql
                    );
                }
                PathSegment::Iif(condition, then_expr, else_expr) => {
                    // iif(condition, then, else) -> CASE WHEN condition THEN then ELSE else END
                    sql = format!(
                        "(CASE WHEN {} THEN {} ELSE {} END)",
                        condition, then_expr, else_expr
                    );
                }
                PathSegment::Distinct => {
                    // distinct() -> SELECT DISTINCT from array elements
                    sql = format!(
                        "(SELECT jsonb_agg(DISTINCT elem) FROM jsonb_array_elements({}) AS elem)",
                        sql
                    );
                }
                PathSegment::Not => {
                    // not() -> boolean negation
                    sql = format!("(NOT {})", sql);
                }
                PathSegment::HasValue => {
                    // hasValue() -> check if value is not null and not empty
                    sql = format!(
                        "({} IS NOT NULL AND {} != 'null'::jsonb AND {} != '\"\"'::jsonb)",
                        sql, sql, sql
                    );
                }
                PathSegment::Matches(pattern) => {
                    // matches(regex) -> PostgreSQL regex match operator
                    let escaped = pattern.replace('\'', "''");
                    sql = format!("({} ~ '{}')", sql, escaped);
                }
            }
        }

        // Apply pending join if any
        if let Some(separator) = pending_join {
            // Use PostgreSQL's jsonb_array_elements_text with string_agg
            sql = format!(
                "(SELECT string_agg(elem, '{}') FROM jsonb_array_elements_text({}) AS elem)",
                separator, sql
            );
        }

        Ok(sql)
    }

    /// Convert a FHIRPath expression to SQL within a forEach context.
    fn fhirpath_to_sql_in_context(
        &self,
        path: &str,
        _table_alias: &str,
        elem_alias: &str,
    ) -> Result<String, Error> {
        if path.is_empty() {
            return Ok(elem_alias.to_string());
        }

        // Parse path into typed segments
        let segments = self.parse_fhirpath_segments(path)?;

        if segments.is_empty() {
            return Ok(elem_alias.to_string());
        }

        self.segments_to_sql(&segments, elem_alias, true)
    }

    /// Convert a FHIRPath expression to a JSONB path for array access.
    fn fhirpath_to_jsonb_array_path(&self, path: &str) -> Result<String, Error> {
        let parts = self.parse_fhirpath(path)?;

        if parts.is_empty() {
            return Err(Error::InvalidPath(format!("Empty forEach path: {}", path)));
        }

        // Build path as a chain of -> operators
        let path_sql = parts
            .iter()
            .map(|p| format!("'{}'", p))
            .collect::<Vec<_>>()
            .join("->");

        Ok(path_sql)
    }

    /// Parse a FHIRPath expression into path segments.
    fn parse_fhirpath(&self, path: &str) -> Result<Vec<String>, Error> {
        let segments = self.parse_fhirpath_segments(path)?;
        // For backward compatibility, extract just field names
        // The full segment info is used by fhirpath_to_sql_with_segments
        Ok(segments
            .into_iter()
            .filter_map(|seg| match seg {
                PathSegment::Field(f) => Some(f),
                _ => None,
            })
            .collect())
    }

    /// Parse a FHIRPath expression into typed segments.
    fn parse_fhirpath_segments(&self, path: &str) -> Result<Vec<PathSegment>, Error> {
        let mut segments = Vec::new();

        // Remove any leading resource type (e.g., "Patient.name" -> "name")
        let path = if let Some(dot_pos) = path.find('.') {
            let first_part = &path[..dot_pos];
            // Check if the first part is a resource type (starts with uppercase)
            if first_part
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                &path[dot_pos + 1..]
            } else {
                path
            }
        } else {
            // Single element - check if it's a resource type
            if path.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !path.contains('(') {
                return Ok(vec![]);
            }
            path
        };

        // Split by '.' but handle function calls and array indexing
        let mut current = String::new();
        let mut paren_depth = 0;
        let mut bracket_depth = 0;

        for c in path.chars() {
            match c {
                '(' => {
                    paren_depth += 1;
                    current.push(c);
                }
                ')' => {
                    paren_depth -= 1;
                    current.push(c);
                }
                '[' => {
                    bracket_depth += 1;
                    current.push(c);
                }
                ']' => {
                    bracket_depth -= 1;
                    current.push(c);
                }
                '.' if paren_depth == 0 && bracket_depth == 0 => {
                    if !current.is_empty() {
                        if let Some(segment) = self.parse_segment(&current)? {
                            segments.push(segment);
                        }
                        current.clear();
                    }
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty()
            && let Some(segment) = self.parse_segment(&current)?
        {
            segments.push(segment);
        }

        Ok(segments)
    }

    /// Parse a single path segment into a PathSegment enum.
    fn parse_segment(&self, segment: &str) -> Result<Option<PathSegment>, Error> {
        // Check for array indexing [n]
        if segment.starts_with('[') && segment.ends_with(']') {
            let index_str = &segment[1..segment.len() - 1];
            let index = index_str
                .parse::<i32>()
                .map_err(|_| Error::InvalidPath(format!("Invalid array index: {}", segment)))?;
            return Ok(Some(PathSegment::Index(index)));
        }

        // Check for function calls
        if let Some(paren_pos) = segment.find('(') {
            let func_name = &segment[..paren_pos];
            let args = &segment[paren_pos + 1..segment.len() - 1]; // Remove ( and )

            return Ok(Some(match func_name {
                "first" => PathSegment::First,
                "last" => PathSegment::Last,
                "exists" => PathSegment::Exists,
                "empty" => PathSegment::Empty,
                "ofType" => PathSegment::OfType(args.trim_matches('\'').to_string()),
                "where" => PathSegment::Where(args.to_string()),
                "extension" => {
                    // Parse the URL from extension('url')
                    let url = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::Extension(url)
                }
                "getReferenceKey" => {
                    // Parse optional resource type from getReferenceKey() or getReferenceKey(Patient)
                    let resource_type = if args.is_empty() {
                        None
                    } else {
                        Some(args.trim_matches('\'').trim_matches('"').to_string())
                    };
                    PathSegment::GetReferenceKey(resource_type)
                }
                "join" => {
                    // Parse the separator from join('separator')
                    let separator = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::Join(separator)
                }
                "contains" => {
                    // Parse the substring from contains('substring')
                    let substring = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::Contains(substring)
                }
                "startsWith" => {
                    // Parse the prefix from startsWith('prefix')
                    let prefix = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::StartsWith(prefix)
                }
                "endsWith" => {
                    // Parse the suffix from endsWith('suffix')
                    let suffix = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::EndsWith(suffix)
                }
                "count" => PathSegment::Count,
                "distinct" => PathSegment::Distinct,
                "not" => PathSegment::Not,
                "hasValue" => PathSegment::HasValue,
                "matches" => {
                    // Parse the regex pattern from matches('pattern')
                    let pattern = args.trim_matches('\'').trim_matches('"').to_string();
                    PathSegment::Matches(pattern)
                }
                "iif" => {
                    // Parse iif(condition, then, else) - simplified parsing
                    // Note: This is a simplified version that doesn't handle nested commas
                    let parts: Vec<&str> = args.splitn(3, ',').collect();
                    if parts.len() >= 2 {
                        let condition = parts[0].trim().to_string();
                        let then_expr = parts[1].trim().trim_matches('\'').to_string();
                        let else_expr = parts
                            .get(2)
                            .map(|s| s.trim().trim_matches('\'').to_string())
                            .unwrap_or_default();
                        PathSegment::Iif(condition, then_expr, else_expr)
                    } else {
                        return Err(Error::InvalidPath(format!(
                            "Invalid iif() arguments: {}",
                            args
                        )));
                    }
                }
                // Skip unknown functions for now - they may be handled elsewhere
                _ => return Ok(None),
            }));
        }

        // Check for field with array index suffix like "name[0]"
        if let Some(bracket_pos) = segment.find('[') {
            let field = &segment[..bracket_pos];
            let index_part = &segment[bracket_pos..];
            if index_part.ends_with(']') {
                let index_str = &index_part[1..index_part.len() - 1];
                if let Ok(index) = index_str.parse::<i32>() {
                    // Return field followed by index
                    // For now, just return the field - we'll handle this in the SQL generation
                    return Ok(Some(PathSegment::Field(format!("{}[{}]", field, index))));
                }
            }
        }

        // Simple field access
        Ok(Some(PathSegment::Field(segment.to_string())))
    }

    /// Create a column alias with optional prefix.
    fn make_column_alias(&self, name: &str, prefix: &str) -> String {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}_{}", prefix, name)
        }
    }

    /// Parse a FHIRPath condition into (field, operator, value).
    ///
    /// Supports conditions like:
    /// - `use = 'official'`
    /// - `system = 'http://loinc.org'`
    /// - `value > 100`
    fn parse_condition(&self, condition: &str) -> Result<(String, String, String), Error> {
        // Order matters: check multi-char operators first
        let operators = ["!=", ">=", "<=", "~", "=", ">", "<"];

        for op in operators {
            if let Some(pos) = condition.find(op) {
                let field = condition[..pos].trim().to_string();
                let value = condition[pos + op.len()..]
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string();

                // Convert FHIRPath operators to SQL operators
                let sql_op = match op {
                    "~" => "LIKE", // FHIRPath contains becomes SQL LIKE
                    _ => op,
                };

                return Ok((field, sql_op.to_string(), value));
            }
        }

        Err(Error::InvalidPath(format!(
            "Cannot parse condition: {}",
            condition
        )))
    }
}

/// Generated SQL with column metadata.
#[derive(Debug, Clone)]
pub struct GeneratedSql {
    /// The generated SQL query.
    pub sql: String,

    /// Column information for the result set.
    pub columns: Vec<GeneratedColumn>,

    /// Common Table Expressions (CTEs) to prepend to the query.
    pub ctes: Vec<String>,
}

/// A generated column with its SQL expression and metadata.
#[derive(Debug, Clone)]
pub struct GeneratedColumn {
    /// Original column name from the ViewDefinition.
    pub name: String,

    /// SQL expression that produces this column's value.
    pub expression: String,

    /// Alias used in the SQL SELECT clause.
    pub alias: String,

    /// Data type of the column.
    pub col_type: ColumnType,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_view(json: serde_json::Value) -> ViewDefinition {
        ViewDefinition::from_json(&json).unwrap()
    }

    #[test]
    fn test_generate_simple_sql() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_demo",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }, {
                    "name": "gender",
                    "path": "gender"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        assert!(result.sql.contains("SELECT"));
        assert!(result.sql.contains("FROM patient"));
        assert!(result.sql.contains("base.id"));
        assert!(result.sql.contains("gender"));
        assert_eq!(result.columns.len(), 2);
    }

    #[test]
    fn test_generate_sql_with_nested_path() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_name",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "family",
                    "path": "name.family"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have nested JSON access
        assert!(result.sql.contains("resource->'name'->>'family'"));
    }

    #[test]
    fn test_generate_sql_with_foreach() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_names",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "forEach": "name",
                "column": [{
                    "name": "family",
                    "path": "family"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have LATERAL join for array expansion
        assert!(result.sql.contains("CROSS JOIN LATERAL"));
        assert!(result.sql.contains("jsonb_array_elements"));
    }

    #[test]
    fn test_generate_sql_with_where() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "active_patients",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }]
            }],
            "where": [{
                "path": "active"
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have WHERE clause
        assert!(result.sql.contains("WHERE"));
        assert!(result.sql.contains("active"));
    }

    #[test]
    fn test_parse_fhirpath_simple() {
        let generator = SqlGenerator::new();

        let parts = generator.parse_fhirpath("name").unwrap();
        assert_eq!(parts, vec!["name"]);

        let parts = generator.parse_fhirpath("name.family").unwrap();
        assert_eq!(parts, vec!["name", "family"]);

        let parts = generator.parse_fhirpath("Patient.name.family").unwrap();
        assert_eq!(parts, vec!["name", "family"]);
    }

    #[test]
    fn test_fhirpath_to_sql() {
        let generator = SqlGenerator::new();

        let sql = generator.fhirpath_to_sql("id", "base").unwrap();
        assert_eq!(sql, "base.id");

        let sql = generator.fhirpath_to_sql("gender", "base").unwrap();
        assert_eq!(sql, "base.resource->>'gender'");

        let sql = generator.fhirpath_to_sql("name.family", "base").unwrap();
        assert_eq!(sql, "base.resource->'name'->>'family'");
    }

    #[test]
    fn test_column_types() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "typed_view",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "birth_date",
                    "path": "birthDate",
                    "type": "date"
                }, {
                    "name": "active",
                    "path": "active",
                    "type": "boolean"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        assert_eq!(result.columns[0].col_type, ColumnType::Date);
        assert_eq!(result.columns[1].col_type, ColumnType::Boolean);
    }

    #[test]
    fn test_fhirpath_first_function() {
        let generator = SqlGenerator::new();

        // Test first() at end of path
        let sql = generator.fhirpath_to_sql("name.first()", "base").unwrap();
        assert_eq!(sql, "base.resource->'name'->0");

        // Test first() in middle of path - the key fix for Patient.name.first().given
        let sql = generator
            .fhirpath_to_sql("name.first().given", "base")
            .unwrap();
        assert_eq!(sql, "base.resource->'name'->0->>'given'");

        // Test first() with nested path before it
        let sql = generator
            .fhirpath_to_sql("contact.name.first()", "base")
            .unwrap();
        assert_eq!(sql, "base.resource->'contact'->'name'->0");
    }

    #[test]
    fn test_fhirpath_last_function() {
        let generator = SqlGenerator::new();

        let sql = generator.fhirpath_to_sql("name.last()", "base").unwrap();
        assert_eq!(sql, "base.resource->'name'->-1");

        let sql = generator
            .fhirpath_to_sql("name.last().family", "base")
            .unwrap();
        assert_eq!(sql, "base.resource->'name'->-1->>'family'");
    }

    #[test]
    fn test_fhirpath_join_function() {
        let generator = SqlGenerator::new();

        // Test join() function - should generate subquery with string_agg
        let sql = generator
            .fhirpath_to_sql("given.join(' ')", "base")
            .unwrap();
        assert!(sql.contains("string_agg"));
        assert!(sql.contains("jsonb_array_elements_text"));
        assert!(sql.contains("' '"));
    }

    #[test]
    fn test_fhirpath_complex_expression() {
        let generator = SqlGenerator::new();

        // Test: name.first().given.join(' ') - common pattern for getting full given name
        let sql = generator
            .fhirpath_to_sql("name.first().given.join(' ')", "base")
            .unwrap();
        assert!(sql.contains("->0")); // first() should be translated
        assert!(sql.contains("string_agg")); // join() should be translated
    }

    #[test]
    fn test_generate_sql_with_first_function() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_first_name",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "family",
                    "path": "name.first().family"
                }, {
                    "name": "given",
                    "path": "name.first().given"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Both should use ->0 for first() translation
        assert!(
            result.sql.contains("->0"),
            "SQL should contain ->0 for first(): {}",
            result.sql
        );
    }

    #[test]
    fn test_parse_fhirpath_segments() {
        let generator = SqlGenerator::new();

        let segments = generator.parse_fhirpath_segments("name").unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], PathSegment::Field("name".to_string()));

        let segments = generator
            .parse_fhirpath_segments("name.first().given")
            .unwrap();
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], PathSegment::Field("name".to_string()));
        assert_eq!(segments[1], PathSegment::First);
        assert_eq!(segments[2], PathSegment::Field("given".to_string()));

        let segments = generator
            .parse_fhirpath_segments("given.join(' ')")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PathSegment::Field("given".to_string()));
        assert_eq!(segments[1], PathSegment::Join(" ".to_string()));
    }

    #[test]
    fn test_fhirpath_where_function() {
        let generator = SqlGenerator::new();

        // Test where() function - should generate subquery with jsonb_agg
        let sql = generator
            .fhirpath_to_sql("name.where(use = 'official')", "base")
            .unwrap();
        assert!(
            sql.contains("jsonb_agg"),
            "SQL should contain jsonb_agg: {}",
            sql
        );
        assert!(
            sql.contains("elem->>'use' = 'official'"),
            "SQL should filter by use: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_where_with_first() {
        let generator = SqlGenerator::new();

        // Test: name.where(use = 'official').first().family
        let sql = generator
            .fhirpath_to_sql("name.where(use = 'official').first().family", "base")
            .unwrap();
        assert!(
            sql.contains("jsonb_agg"),
            "SQL should contain jsonb_agg: {}",
            sql
        );
        assert!(
            sql.contains("->0"),
            "SQL should contain ->0 for first(): {}",
            sql
        );
        assert!(sql.contains("family"), "SQL should contain family: {}", sql);
    }

    #[test]
    fn test_fhirpath_extension_function() {
        let generator = SqlGenerator::new();

        // Test extension() function
        let sql = generator
            .fhirpath_to_sql("extension('http://example.org/race')", "base")
            .unwrap();
        assert!(
            sql.contains("elem->>'url' = 'http://example.org/race'"),
            "SQL should filter by URL: {}",
            sql
        );
        assert!(
            sql.contains("jsonb_array_elements"),
            "SQL should use jsonb_array_elements: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_extension_with_value() {
        let generator = SqlGenerator::new();

        // Test extension().valueCoding.code
        let sql = generator
            .fhirpath_to_sql(
                "extension('http://example.org/race').valueCoding.code",
                "base",
            )
            .unwrap();
        assert!(
            sql.contains("elem->>'url' = 'http://example.org/race'"),
            "SQL should filter by URL: {}",
            sql
        );
        assert!(
            sql.contains("valueCoding"),
            "SQL should contain valueCoding: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_get_reference_key() {
        let generator = SqlGenerator::new();

        // Test getReferenceKey() without type
        let sql = generator
            .fhirpath_to_sql("subject.getReferenceKey()", "base")
            .unwrap();
        assert!(
            sql.contains("fhir_ref_id"),
            "SQL should use fhir_ref_id: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_get_reference_key_with_type() {
        let generator = SqlGenerator::new();

        // Test getReferenceKey(Patient) with type filter
        let sql = generator
            .fhirpath_to_sql("subject.getReferenceKey(Patient)", "base")
            .unwrap();
        assert!(
            sql.contains("fhir_ref_type"),
            "SQL should use fhir_ref_type for type check: {}",
            sql
        );
        assert!(
            sql.contains("Patient"),
            "SQL should filter by Patient type: {}",
            sql
        );
        assert!(
            sql.contains("CASE WHEN"),
            "SQL should use CASE WHEN: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_empty_function() {
        let generator = SqlGenerator::new();

        // Test empty() function
        let sql = generator.fhirpath_to_sql("name.empty()", "base").unwrap();
        assert!(
            sql.contains("jsonb_array_length"),
            "SQL should use jsonb_array_length: {}",
            sql
        );
        assert!(
            sql.contains("CASE WHEN"),
            "SQL should use CASE WHEN: {}",
            sql
        );
        assert!(
            sql.contains("IS NULL"),
            "SQL should check for NULL: {}",
            sql
        );
    }

    #[test]
    fn test_parse_condition() {
        let generator = SqlGenerator::new();

        let (field, op, value) = generator.parse_condition("use = 'official'").unwrap();
        assert_eq!(field, "use");
        assert_eq!(op, "=");
        assert_eq!(value, "official");

        let (field, op, value) = generator.parse_condition("value > 100").unwrap();
        assert_eq!(field, "value");
        assert_eq!(op, ">");
        assert_eq!(value, "100");

        let (field, op, value) = generator.parse_condition("code != 'inactive'").unwrap();
        assert_eq!(field, "code");
        assert_eq!(op, "!=");
        assert_eq!(value, "inactive");
    }

    #[test]
    fn test_parse_fhirpath_segments_new_functions() {
        let generator = SqlGenerator::new();

        // Test extension parsing
        let segments = generator
            .parse_fhirpath_segments("extension('http://example.org/ext')")
            .unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0],
            PathSegment::Extension("http://example.org/ext".to_string())
        );

        // Test getReferenceKey parsing without type
        let segments = generator
            .parse_fhirpath_segments("subject.getReferenceKey()")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PathSegment::Field("subject".to_string()));
        assert_eq!(segments[1], PathSegment::GetReferenceKey(None));

        // Test getReferenceKey parsing with type
        let segments = generator
            .parse_fhirpath_segments("subject.getReferenceKey(Patient)")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PathSegment::Field("subject".to_string()));
        assert_eq!(
            segments[1],
            PathSegment::GetReferenceKey(Some("Patient".to_string()))
        );

        // Test empty parsing
        let segments = generator.parse_fhirpath_segments("name.empty()").unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PathSegment::Field("name".to_string()));
        assert_eq!(segments[1], PathSegment::Empty);
    }

    #[test]
    fn test_constant_substitution() {
        // Test constant substitution in column paths
        let generator = SqlGenerator::new();

        // Test the substitute_constants method directly
        let mut constants = std::collections::HashMap::new();
        constants.insert("targetSystem".to_string(), "'http://loinc.org'".to_string());
        constants.insert("maxValue".to_string(), "100".to_string());

        let path = "code.coding.where(system = %targetSystem)";
        let substituted = generator.substitute_constants(path, &constants);
        assert_eq!(
            substituted, "code.coding.where(system = 'http://loinc.org')",
            "Constants should be substituted in path"
        );

        // Test integer constant
        let path = "value > %maxValue";
        let substituted = generator.substitute_constants(path, &constants);
        assert_eq!(
            substituted, "value > 100",
            "Integer constants should be substituted"
        );
    }

    #[test]
    fn test_type_casting() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "typed_view",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "birth_date",
                    "path": "birthDate",
                    "type": "date"
                }, {
                    "name": "active",
                    "path": "active",
                    "type": "boolean"
                }, {
                    "name": "age",
                    "path": "extension.valueInteger",
                    "type": "integer"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Check that type casts are applied
        assert!(
            result.sql.contains("::date"),
            "SQL should contain date cast: {}",
            result.sql
        );
        assert!(
            result.sql.contains("::boolean"),
            "SQL should contain boolean cast: {}",
            result.sql
        );
        assert!(
            result.sql.contains("::bigint"),
            "SQL should contain bigint cast: {}",
            result.sql
        );
    }

    #[test]
    fn test_fhirpath_contains_function() {
        let generator = SqlGenerator::new();

        let sql = generator
            .fhirpath_to_sql("name.contains('John')", "base")
            .unwrap();
        assert!(
            sql.contains("LIKE '%John%'"),
            "SQL should use LIKE pattern: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_startswith_function() {
        let generator = SqlGenerator::new();

        let sql = generator
            .fhirpath_to_sql("name.startsWith('Dr')", "base")
            .unwrap();
        assert!(
            sql.contains("LIKE 'Dr%'"),
            "SQL should use LIKE prefix pattern: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_endswith_function() {
        let generator = SqlGenerator::new();

        let sql = generator
            .fhirpath_to_sql("name.endsWith('son')", "base")
            .unwrap();
        assert!(
            sql.contains("LIKE '%son'"),
            "SQL should use LIKE suffix pattern: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_count_function() {
        let generator = SqlGenerator::new();

        let sql = generator.fhirpath_to_sql("name.count()", "base").unwrap();
        assert!(
            sql.contains("jsonb_array_length"),
            "SQL should use jsonb_array_length: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_distinct_function() {
        let generator = SqlGenerator::new();

        let sql = generator
            .fhirpath_to_sql("identifier.distinct()", "base")
            .unwrap();
        assert!(sql.contains("DISTINCT"), "SQL should use DISTINCT: {}", sql);
        assert!(
            sql.contains("jsonb_agg"),
            "SQL should use jsonb_agg: {}",
            sql
        );
    }

    #[test]
    fn test_fhirpath_not_function() {
        let generator = SqlGenerator::new();

        let sql = generator.fhirpath_to_sql("active.not()", "base").unwrap();
        assert!(sql.contains("NOT"), "SQL should contain NOT: {}", sql);
    }

    #[test]
    fn test_fhirpath_hasvalue_function() {
        let generator = SqlGenerator::new();

        let sql = generator
            .fhirpath_to_sql("gender.hasValue()", "base")
            .unwrap();
        assert!(
            sql.contains("IS NOT NULL"),
            "SQL should check IS NOT NULL: {}",
            sql
        );
    }

    #[test]
    fn test_parse_new_fhirpath_functions() {
        let generator = SqlGenerator::new();

        // Test contains parsing
        let segments = generator
            .parse_fhirpath_segments("name.contains('test')")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1], PathSegment::Contains("test".to_string()));

        // Test startsWith parsing
        let segments = generator
            .parse_fhirpath_segments("name.startsWith('Dr')")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1], PathSegment::StartsWith("Dr".to_string()));

        // Test count parsing
        let segments = generator.parse_fhirpath_segments("name.count()").unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1], PathSegment::Count);

        // Test distinct parsing
        let segments = generator
            .parse_fhirpath_segments("identifier.distinct()")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1], PathSegment::Distinct);
    }

    #[test]
    fn test_fhirpath_matches_function() {
        let generator = SqlGenerator::new();

        // Test matches() function with regex pattern
        let sql = generator
            .fhirpath_to_sql("name.family.matches('[A-Z].*')", "base")
            .unwrap();
        assert!(
            sql.contains("~"),
            "SQL should use regex operator ~: {}",
            sql
        );
        assert!(
            sql.contains("[A-Z].*"),
            "SQL should contain regex pattern: {}",
            sql
        );
    }

    #[test]
    fn test_parse_matches_function() {
        let generator = SqlGenerator::new();

        // Test matches parsing
        let segments = generator
            .parse_fhirpath_segments("family.matches('[A-Z][a-z]+')")
            .unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PathSegment::Field("family".to_string()));
        assert_eq!(segments[1], PathSegment::Matches("[A-Z][a-z]+".to_string()));
    }

    #[test]
    fn test_repeat_expression_generates_recursive_cte() {
        // Test repeat expression generates recursive CTEs for hierarchical data
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "nested_extensions",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "forEach": "extension",
                "repeat": ["extension"],
                "column": [{
                    "name": "url",
                    "path": "url"
                }, {
                    "name": "value",
                    "path": "valueString"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should generate a recursive CTE
        assert!(
            result.sql.contains("WITH RECURSIVE"),
            "SQL should contain WITH RECURSIVE: {}",
            result.sql
        );
        assert!(
            result.sql.contains("repeat_cte_"),
            "SQL should contain repeat CTE name: {}",
            result.sql
        );
        assert!(
            result.sql.contains("UNION ALL"),
            "SQL should contain UNION ALL for recursion: {}",
            result.sql
        );
        // Should have depth limit to prevent infinite recursion
        assert!(
            result.sql.contains("depth < 10"),
            "SQL should have depth limit: {}",
            result.sql
        );
        // CTEs should be stored
        assert!(
            !result.ctes.is_empty(),
            "Result should have CTEs: {:?}",
            result.ctes
        );
    }

    #[test]
    fn test_repeat_expression_with_for_each_or_null() {
        // Test repeat with forEachOrNull (optional array expansion)
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "optional_nested_extensions",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "forEachOrNull": "extension",
                "repeat": ["extension"],
                "column": [{
                    "name": "url",
                    "path": "url"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should still generate a recursive CTE
        assert!(
            result.sql.contains("WITH RECURSIVE"),
            "SQL should contain WITH RECURSIVE: {}",
            result.sql
        );
        assert!(
            result.sql.contains("UNION ALL"),
            "SQL should contain UNION ALL: {}",
            result.sql
        );
    }
}
