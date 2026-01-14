//! Schema cache for PostgreSQL database introspection.
//!
//! This module caches database schema information (tables, columns, functions)
//! for providing intelligent completions.

use dashmap::DashMap;
use sqlx_core::error::Error as SqlxError;
use sqlx_core::query_as::query_as;
use std::sync::Arc;

/// Information about a database table.
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Schema name (e.g., "public")
    pub schema: String,
    /// Table name
    pub name: String,
    /// Table type (BASE TABLE, VIEW, etc.)
    pub table_type: String,
    /// Whether this is a FHIR resource table
    pub is_fhir_table: bool,
    /// FHIR resource type if applicable (e.g., "Patient")
    pub fhir_resource_type: Option<String>,
}

/// Information about a database column.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Schema name this column belongs to
    pub schema: String,
    /// Table name this column belongs to
    pub table_name: String,
    /// Column name
    pub name: String,
    /// Data type (e.g., "jsonb", "text", "integer")
    pub data_type: String,
    /// Whether the column is nullable
    pub is_nullable: bool,
    /// Column default value if any
    pub default_value: Option<String>,
    /// Column description/comment if any
    pub description: Option<String>,
}

/// Information about a database function.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name
    pub name: String,
    /// Return type
    pub return_type: String,
    /// Function signature/arguments
    pub signature: String,
    /// Description
    pub description: String,
}

/// Information about a database schema.
#[derive(Debug, Clone)]
pub struct SchemaInfo {
    /// Schema name (e.g., "public", "auth")
    pub name: String,
    /// Whether this is a user-created schema (vs system schema like pg_catalog)
    pub is_user_schema: bool,
}

/// Information about a row-level security policy.
#[derive(Debug, Clone)]
pub struct PolicyInfo {
    /// Policy name
    pub name: String,
    /// Schema name
    pub schema: String,
    /// Table name the policy applies to
    pub table_name: String,
    /// Policy command (ALL, SELECT, INSERT, UPDATE, DELETE)
    pub command: String,
    /// Policy permissiveness (PERMISSIVE or RESTRICTIVE)
    pub permissive: bool,
    /// Roles the policy applies to
    pub roles: Vec<String>,
    /// USING expression
    pub using_expr: Option<String>,
    /// WITH CHECK expression
    pub with_check_expr: Option<String>,
}

/// Information about a database role.
#[derive(Debug, Clone)]
pub struct RoleInfo {
    /// Role name
    pub name: String,
    /// Whether the role can log in
    pub can_login: bool,
    /// Whether the role is a superuser
    pub is_superuser: bool,
    /// Whether the role can create databases
    pub create_db: bool,
    /// Whether the role can create other roles
    pub create_role: bool,
    /// Roles this role inherits from
    pub member_of: Vec<String>,
}

/// Information about a PostgreSQL type.
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Type name
    pub name: String,
    /// Schema name
    pub schema: String,
    /// Type category (Enum, Composite, Range, Domain, Base)
    pub category: String,
    /// For enum types, the labels
    pub enum_labels: Vec<String>,
    /// For composite types, the attributes
    pub attributes: Vec<(String, String)>, // (name, type)
    /// Description/comment
    pub description: Option<String>,
}

/// Schema cache manager.
pub struct SchemaCache {
    /// Cached schemas by name
    schemas: DashMap<String, SchemaInfo>,
    /// Cached tables by schema.table_name
    tables: DashMap<String, TableInfo>,
    /// Cached columns by table_name
    columns: DashMap<String, Vec<ColumnInfo>>,
    /// Cached functions by name
    functions: DashMap<String, FunctionInfo>,
    /// Cached policies by schema.table.policy_name
    policies: DashMap<String, PolicyInfo>,
    /// Cached roles by name
    roles: DashMap<String, RoleInfo>,
    /// Cached types by schema.type_name
    types: DashMap<String, TypeInfo>,
    /// Database connection pool
    db_pool: Arc<sqlx_postgres::PgPool>,
}

impl SchemaCache {
    /// Creates a new schema cache.
    pub fn new(db_pool: Arc<sqlx_postgres::PgPool>) -> Self {
        Self {
            schemas: DashMap::new(),
            tables: DashMap::new(),
            columns: DashMap::new(),
            functions: DashMap::new(),
            policies: DashMap::new(),
            roles: DashMap::new(),
            types: DashMap::new(),
            db_pool,
        }
    }

    /// Refresh the schema cache from the database.
    pub async fn refresh(&self) -> Result<(), SqlxError> {
        self.refresh_schemas().await?;
        self.refresh_tables().await?;
        self.refresh_columns().await?;
        self.refresh_functions().await?;
        self.refresh_policies().await?;
        self.refresh_roles().await?;
        self.refresh_types().await?;
        self.load_builtin_functions();
        Ok(())
    }

    /// Refresh schema information from information_schema.
    async fn refresh_schemas(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String,)>(
            r#"
            SELECT schema_name
            FROM information_schema.schemata
            ORDER BY schema_name
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.schemas.clear();

        for (schema_name,) in rows {
            // User schemas are those that are not system schemas
            let is_user_schema =
                !schema_name.starts_with("pg_") && schema_name != "information_schema";

            self.schemas.insert(
                schema_name.clone(),
                SchemaInfo {
                    name: schema_name,
                    is_user_schema,
                },
            );
        }

        tracing::info!(
            total = self.schemas.len(),
            user = self.schemas.iter().filter(|s| s.is_user_schema).count(),
            "LSP schema cache: loaded schemas"
        );
        Ok(())
    }

    /// Refresh table information from information_schema.
    async fn refresh_tables(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String, String, String)>(
            r#"
            SELECT table_schema, table_name, table_type
            FROM information_schema.tables
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY table_schema, table_name
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.tables.clear();

        for (schema, name, table_type) in rows {
            // Check if this looks like a FHIR resource table
            let (is_fhir, resource_type) = Self::detect_fhir_table(&name);

            let key = format!("{}.{}", schema, name);
            self.tables.insert(
                key,
                TableInfo {
                    schema,
                    name,
                    table_type,
                    is_fhir_table: is_fhir,
                    fhir_resource_type: resource_type,
                },
            );
        }

        let fhir_count = self.tables.iter().filter(|t| t.is_fhir_table).count();
        tracing::info!(
            total = self.tables.len(),
            fhir = fhir_count,
            "LSP schema cache: loaded tables"
        );
        Ok(())
    }

    /// Refresh column information from information_schema.
    async fn refresh_columns(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String, String, String, String, String, Option<String>)>(
            r#"
            SELECT
                table_schema,
                table_name,
                column_name,
                data_type,
                is_nullable,
                column_default
            FROM information_schema.columns
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY table_schema, table_name, ordinal_position
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.columns.clear();

        for (schema, table_name, column_name, data_type, is_nullable, default_value) in rows {
            let column = ColumnInfo {
                schema: schema.clone(),
                table_name: table_name.clone(),
                name: column_name,
                data_type,
                is_nullable: is_nullable == "YES",
                default_value,
                description: None,
            };

            self.columns
                .entry(Self::column_key(&schema, &table_name))
                .or_default()
                .push(column);
        }

        let total_columns: usize = self.columns.iter().map(|r| r.value().len()).sum();
        tracing::info!(
            tables = self.columns.len(),
            columns = total_columns,
            "LSP schema cache: loaded columns"
        );
        Ok(())
    }

    /// Refresh row-level security policies from pg_policies.
    async fn refresh_policies(&self) -> Result<(), SqlxError> {
        let rows = query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                bool,
                Vec<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            r#"
            SELECT
                policyname,
                schemaname,
                tablename,
                cmd,
                permissive = 'PERMISSIVE',
                roles::text[]::text[] as roles,
                qual,
                with_check
            FROM pg_policies
            ORDER BY schemaname, tablename, policyname
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.policies.clear();

        for (name, schema, table_name, command, permissive, roles, using_expr, with_check_expr) in
            rows
        {
            let key = format!("{}.{}.{}", schema, table_name, name);
            self.policies.insert(
                key,
                PolicyInfo {
                    name,
                    schema,
                    table_name,
                    command,
                    permissive,
                    roles,
                    using_expr,
                    with_check_expr,
                },
            );
        }

        tracing::info!(
            count = self.policies.len(),
            "LSP schema cache: loaded policies"
        );
        Ok(())
    }

    /// Refresh database roles from pg_roles.
    async fn refresh_roles(&self) -> Result<(), SqlxError> {
        // Get basic role info
        let rows = query_as::<_, (String, bool, bool, bool, bool)>(
            r#"
            SELECT
                rolname,
                rolcanlogin,
                rolsuper,
                rolcreatedb,
                rolcreaterole
            FROM pg_roles
            WHERE rolname NOT LIKE 'pg_%'
            ORDER BY rolname
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.roles.clear();

        for (name, can_login, is_superuser, create_db, create_role) in rows {
            self.roles.insert(
                name.clone(),
                RoleInfo {
                    name,
                    can_login,
                    is_superuser,
                    create_db,
                    create_role,
                    member_of: Vec::new(), // Could be populated with another query if needed
                },
            );
        }

        tracing::info!(count = self.roles.len(), "LSP schema cache: loaded roles");
        Ok(())
    }

    /// Refresh custom types from pg_type.
    async fn refresh_types(&self) -> Result<(), SqlxError> {
        // Get enum types with their labels
        let enum_rows = query_as::<_, (String, String, Vec<String>)>(
            r#"
            SELECT
                t.typname,
                n.nspname,
                ARRAY(
                    SELECT enumlabel::text
                    FROM pg_enum e
                    WHERE e.enumtypid = t.oid
                    ORDER BY e.enumsortorder
                ) as labels
            FROM pg_type t
            JOIN pg_namespace n ON t.typnamespace = n.oid
            WHERE t.typtype = 'e'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema')
            ORDER BY n.nspname, t.typname
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.types.clear();

        for (name, schema, enum_labels) in enum_rows {
            let key = format!("{}.{}", schema, name);
            self.types.insert(
                key,
                TypeInfo {
                    name,
                    schema,
                    category: "Enum".to_string(),
                    enum_labels,
                    attributes: Vec::new(),
                    description: None,
                },
            );
        }

        // Get composite types
        let composite_rows = query_as::<_, (String, String)>(
            r#"
            SELECT
                t.typname,
                n.nspname
            FROM pg_type t
            JOIN pg_namespace n ON t.typnamespace = n.oid
            WHERE t.typtype = 'c'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema')
              AND t.typname NOT LIKE '%_seq'
            ORDER BY n.nspname, t.typname
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        for (name, schema) in composite_rows {
            let key = format!("{}.{}", schema, name);
            // Skip if already exists (e.g., from enum query)
            if !self.types.contains_key(&key) {
                self.types.insert(
                    key,
                    TypeInfo {
                        name,
                        schema,
                        category: "Composite".to_string(),
                        enum_labels: Vec::new(),
                        attributes: Vec::new(), // Could be populated with another query if needed
                        description: None,
                    },
                );
            }
        }

        tracing::info!(count = self.types.len(), "LSP schema cache: loaded types");
        Ok(())
    }

    /// Load built-in PostgreSQL functions.
    fn load_builtin_functions(&self) {
        // Format: (name, return_type, signature, description)
        const FUNCTIONS: &[(&str, &str, &str, &str)] = &[
            // === JSONB Functions ===
            (
                "jsonb_extract_path",
                "jsonb",
                "jsonb_extract_path(from_json jsonb, VARIADIC path_elems text[])",
                "Extract JSON sub-object at path",
            ),
            (
                "jsonb_extract_path_text",
                "text",
                "jsonb_extract_path_text(from_json jsonb, VARIADIC path_elems text[])",
                "Extract JSON sub-object as text",
            ),
            (
                "jsonb_array_elements",
                "setof jsonb",
                "jsonb_array_elements(from_json jsonb)",
                "Expand JSONB array to set of rows",
            ),
            (
                "jsonb_array_elements_text",
                "setof text",
                "jsonb_array_elements_text(from_json jsonb)",
                "Expand JSONB array as text rows",
            ),
            (
                "jsonb_object_keys",
                "setof text",
                "jsonb_object_keys(from_json jsonb)",
                "Get set of keys in outermost object",
            ),
            (
                "jsonb_typeof",
                "text",
                "jsonb_typeof(from_json jsonb)",
                "Get type of outermost JSON value",
            ),
            (
                "jsonb_agg",
                "jsonb",
                "jsonb_agg(expression anyelement)",
                "Aggregate values as JSONB array",
            ),
            (
                "jsonb_build_object",
                "jsonb",
                "jsonb_build_object(VARIADIC args \"any\")",
                "Build JSONB object from arguments",
            ),
            (
                "jsonb_build_array",
                "jsonb",
                "jsonb_build_array(VARIADIC args \"any\")",
                "Build JSONB array from arguments",
            ),
            (
                "jsonb_set",
                "jsonb",
                "jsonb_set(target jsonb, path text[], new_value jsonb [, create_if_missing boolean])",
                "Set value at path",
            ),
            (
                "jsonb_insert",
                "jsonb",
                "jsonb_insert(target jsonb, path text[], new_value jsonb [, insert_after boolean])",
                "Insert value at path",
            ),
            (
                "jsonb_path_query",
                "setof jsonb",
                "jsonb_path_query(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "Execute JSONPath query",
            ),
            (
                "jsonb_path_query_array",
                "jsonb",
                "jsonb_path_query_array(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "JSONPath query as array",
            ),
            (
                "jsonb_path_query_first",
                "jsonb",
                "jsonb_path_query_first(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "First JSONPath result",
            ),
            (
                "jsonb_path_exists",
                "boolean",
                "jsonb_path_exists(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "Check if JSONPath returns items",
            ),
            (
                "jsonb_path_match",
                "boolean",
                "jsonb_path_match(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "Check if JSONPath matches predicate",
            ),
            (
                "jsonb_strip_nulls",
                "jsonb",
                "jsonb_strip_nulls(from_json jsonb)",
                "Remove null values recursively",
            ),
            (
                "jsonb_pretty",
                "text",
                "jsonb_pretty(from_json jsonb)",
                "Pretty print JSONB",
            ),
            (
                "jsonb_each",
                "setof record",
                "jsonb_each(from_json jsonb)",
                "Expand to key-value pairs",
            ),
            (
                "jsonb_each_text",
                "setof record",
                "jsonb_each_text(from_json jsonb)",
                "Expand to key-text pairs",
            ),
            (
                "jsonb_populate_record",
                "anyelement",
                "jsonb_populate_record(base anyelement, from_json jsonb)",
                "Populate record from JSONB",
            ),
            (
                "jsonb_to_record",
                "record",
                "jsonb_to_record(from_json jsonb)",
                "Convert JSONB to record",
            ),
            (
                "to_jsonb",
                "jsonb",
                "to_jsonb(anyelement)",
                "Convert to JSONB",
            ),
            (
                "jsonb_array_length",
                "integer",
                "jsonb_array_length(from_json jsonb)",
                "Get length of JSONB array",
            ),
            (
                "jsonb_object",
                "jsonb",
                "jsonb_object(keys text[], values text[])",
                "Build JSONB object from arrays",
            ),
            // === Aggregate Functions ===
            (
                "count",
                "bigint",
                "count(*) | count(expression)",
                "Count rows or non-null values",
            ),
            (
                "sum",
                "numeric",
                "sum(expression)",
                "Sum of all input values",
            ),
            (
                "avg",
                "numeric",
                "avg(expression)",
                "Average of all input values",
            ),
            ("min", "any", "min(expression)", "Minimum value"),
            ("max", "any", "max(expression)", "Maximum value"),
            (
                "array_agg",
                "anyarray",
                "array_agg(expression [ORDER BY ...])",
                "Aggregate values into array",
            ),
            (
                "string_agg",
                "text",
                "string_agg(expression, delimiter [ORDER BY ...])",
                "Concatenate strings with delimiter",
            ),
            (
                "bool_and",
                "boolean",
                "bool_and(expression)",
                "True if all values are true",
            ),
            (
                "bool_or",
                "boolean",
                "bool_or(expression)",
                "True if any value is true",
            ),
            (
                "every",
                "boolean",
                "every(expression)",
                "Equivalent to bool_and",
            ),
            (
                "bit_and",
                "bit/integer",
                "bit_and(expression)",
                "Bitwise AND of all values",
            ),
            (
                "bit_or",
                "bit/integer",
                "bit_or(expression)",
                "Bitwise OR of all values",
            ),
            // === String Functions ===
            (
                "concat",
                "text",
                "concat(str1, str2, ...)",
                "Concatenate strings",
            ),
            (
                "concat_ws",
                "text",
                "concat_ws(separator, str1, str2, ...)",
                "Concatenate with separator",
            ),
            ("length", "integer", "length(string)", "Length of string"),
            (
                "char_length",
                "integer",
                "char_length(string)",
                "Number of characters",
            ),
            ("lower", "text", "lower(string)", "Convert to lowercase"),
            ("upper", "text", "upper(string)", "Convert to uppercase"),
            (
                "initcap",
                "text",
                "initcap(string)",
                "Capitalize first letter of each word",
            ),
            (
                "trim",
                "text",
                "trim([leading|trailing|both] [characters] FROM string)",
                "Remove characters from string",
            ),
            (
                "ltrim",
                "text",
                "ltrim(string [, characters])",
                "Remove leading characters",
            ),
            (
                "rtrim",
                "text",
                "rtrim(string [, characters])",
                "Remove trailing characters",
            ),
            ("left", "text", "left(string, n)", "First n characters"),
            ("right", "text", "right(string, n)", "Last n characters"),
            (
                "substring",
                "text",
                "substring(string FROM start [FOR count])",
                "Extract substring",
            ),
            (
                "substr",
                "text",
                "substr(string, start [, count])",
                "Extract substring",
            ),
            (
                "position",
                "integer",
                "position(substring IN string)",
                "Location of substring",
            ),
            (
                "strpos",
                "integer",
                "strpos(string, substring)",
                "Location of substring",
            ),
            (
                "replace",
                "text",
                "replace(string, from, to)",
                "Replace occurrences",
            ),
            (
                "regexp_replace",
                "text",
                "regexp_replace(string, pattern, replacement [, flags])",
                "Replace using regex",
            ),
            (
                "regexp_matches",
                "setof text[]",
                "regexp_matches(string, pattern [, flags])",
                "Return regex matches",
            ),
            (
                "regexp_split_to_array",
                "text[]",
                "regexp_split_to_array(string, pattern [, flags])",
                "Split string by regex",
            ),
            (
                "regexp_split_to_table",
                "setof text",
                "regexp_split_to_table(string, pattern [, flags])",
                "Split string to rows",
            ),
            (
                "split_part",
                "text",
                "split_part(string, delimiter, n)",
                "Split string and return nth part",
            ),
            (
                "repeat",
                "text",
                "repeat(string, number)",
                "Repeat string n times",
            ),
            ("reverse", "text", "reverse(string)", "Reverse string"),
            (
                "format",
                "text",
                "format(formatstr, ...)",
                "Format string like printf",
            ),
            (
                "quote_ident",
                "text",
                "quote_ident(string)",
                "Quote identifier for SQL",
            ),
            (
                "quote_literal",
                "text",
                "quote_literal(string)",
                "Quote literal for SQL",
            ),
            (
                "quote_nullable",
                "text",
                "quote_nullable(string)",
                "Quote nullable literal",
            ),
            ("md5", "text", "md5(string)", "MD5 hash as hex"),
            (
                "encode",
                "text",
                "encode(data bytea, format text)",
                "Encode binary to text",
            ),
            (
                "decode",
                "bytea",
                "decode(string text, format text)",
                "Decode text to binary",
            ),
            // === Numeric Functions ===
            ("abs", "numeric", "abs(x)", "Absolute value"),
            ("ceil", "numeric", "ceil(x)", "Round up to integer"),
            ("ceiling", "numeric", "ceiling(x)", "Round up to integer"),
            ("floor", "numeric", "floor(x)", "Round down to integer"),
            (
                "round",
                "numeric",
                "round(x [, s])",
                "Round to s decimal places",
            ),
            (
                "trunc",
                "numeric",
                "trunc(x [, s])",
                "Truncate to s decimal places",
            ),
            ("mod", "numeric", "mod(y, x)", "Remainder of y/x"),
            ("power", "double", "power(a, b)", "a raised to power b"),
            ("sqrt", "double", "sqrt(x)", "Square root"),
            ("cbrt", "double", "cbrt(x)", "Cube root"),
            ("exp", "double", "exp(x)", "Exponential"),
            ("ln", "double", "ln(x)", "Natural logarithm"),
            ("log", "double", "log(x) | log(base, x)", "Logarithm"),
            ("log10", "double", "log10(x)", "Base 10 logarithm"),
            (
                "random",
                "double",
                "random()",
                "Random value 0.0 <= x < 1.0",
            ),
            ("setseed", "void", "setseed(seed double)", "Set random seed"),
            ("sign", "integer", "sign(x)", "Sign of argument (-1, 0, 1)"),
            ("div", "integer", "div(y, x)", "Integer quotient of y/x"),
            (
                "greatest",
                "any",
                "greatest(value1, value2, ...)",
                "Largest value",
            ),
            (
                "least",
                "any",
                "least(value1, value2, ...)",
                "Smallest value",
            ),
            // === Date/Time Functions ===
            (
                "now",
                "timestamp with time zone",
                "now()",
                "Current date and time",
            ),
            (
                "current_timestamp",
                "timestamp with time zone",
                "current_timestamp",
                "Current date and time",
            ),
            ("current_date", "date", "current_date", "Current date"),
            (
                "current_time",
                "time with time zone",
                "current_time",
                "Current time",
            ),
            ("localtime", "time", "localtime", "Current local time"),
            (
                "localtimestamp",
                "timestamp",
                "localtimestamp",
                "Current local timestamp",
            ),
            (
                "age",
                "interval",
                "age(timestamp [, timestamp])",
                "Subtract timestamps",
            ),
            (
                "date_part",
                "double",
                "date_part(field, source)",
                "Extract date/time field",
            ),
            (
                "extract",
                "double",
                "extract(field FROM source)",
                "Extract date/time field",
            ),
            (
                "date_trunc",
                "timestamp",
                "date_trunc(field, source)",
                "Truncate to precision",
            ),
            (
                "to_char",
                "text",
                "to_char(timestamp, format)",
                "Convert to formatted string",
            ),
            (
                "to_date",
                "date",
                "to_date(text, format)",
                "Convert string to date",
            ),
            (
                "to_timestamp",
                "timestamp",
                "to_timestamp(text, format)",
                "Convert string to timestamp",
            ),
            (
                "make_date",
                "date",
                "make_date(year, month, day)",
                "Create date from parts",
            ),
            (
                "make_time",
                "time",
                "make_time(hour, min, sec)",
                "Create time from parts",
            ),
            (
                "make_timestamp",
                "timestamp",
                "make_timestamp(year, month, day, hour, min, sec)",
                "Create timestamp",
            ),
            (
                "make_interval",
                "interval",
                "make_interval(years, months, weeks, days, hours, mins, secs)",
                "Create interval",
            ),
            (
                "clock_timestamp",
                "timestamp with time zone",
                "clock_timestamp()",
                "Current timestamp (changes during statement)",
            ),
            (
                "statement_timestamp",
                "timestamp with time zone",
                "statement_timestamp()",
                "Start of current statement",
            ),
            (
                "transaction_timestamp",
                "timestamp with time zone",
                "transaction_timestamp()",
                "Start of current transaction",
            ),
            (
                "timeofday",
                "text",
                "timeofday()",
                "Current date and time as text",
            ),
            // === Array Functions ===
            (
                "array_append",
                "anyarray",
                "array_append(array, element)",
                "Append element to array",
            ),
            (
                "array_prepend",
                "anyarray",
                "array_prepend(element, array)",
                "Prepend element to array",
            ),
            (
                "array_cat",
                "anyarray",
                "array_cat(array1, array2)",
                "Concatenate arrays",
            ),
            (
                "array_ndims",
                "integer",
                "array_ndims(array)",
                "Number of dimensions",
            ),
            (
                "array_dims",
                "text",
                "array_dims(array)",
                "Text representation of dimensions",
            ),
            (
                "array_length",
                "integer",
                "array_length(array, dimension)",
                "Length of dimension",
            ),
            (
                "array_lower",
                "integer",
                "array_lower(array, dimension)",
                "Lower bound of dimension",
            ),
            (
                "array_upper",
                "integer",
                "array_upper(array, dimension)",
                "Upper bound of dimension",
            ),
            (
                "array_position",
                "integer",
                "array_position(array, element [, start])",
                "Position of element",
            ),
            (
                "array_positions",
                "integer[]",
                "array_positions(array, element)",
                "All positions of element",
            ),
            (
                "array_remove",
                "anyarray",
                "array_remove(array, element)",
                "Remove all occurrences",
            ),
            (
                "array_replace",
                "anyarray",
                "array_replace(array, from, to)",
                "Replace occurrences",
            ),
            (
                "array_to_string",
                "text",
                "array_to_string(array, delimiter [, null_string])",
                "Convert to string",
            ),
            (
                "string_to_array",
                "text[]",
                "string_to_array(string, delimiter [, null_string])",
                "Split string to array",
            ),
            (
                "unnest",
                "setof anyelement",
                "unnest(array)",
                "Expand array to rows",
            ),
            (
                "cardinality",
                "integer",
                "cardinality(array)",
                "Total number of elements",
            ),
            // === Conditional Functions ===
            (
                "coalesce",
                "any",
                "coalesce(value1, value2, ...)",
                "First non-null value",
            ),
            (
                "nullif",
                "any",
                "nullif(value1, value2)",
                "Null if values equal",
            ),
            (
                "greatest",
                "any",
                "greatest(value1, value2, ...)",
                "Largest value",
            ),
            (
                "least",
                "any",
                "least(value1, value2, ...)",
                "Smallest value",
            ),
            // === Type Conversion ===
            ("cast", "any", "cast(value AS type)", "Convert to type"),
            (
                "to_char",
                "text",
                "to_char(value, format)",
                "Convert to formatted string",
            ),
            (
                "to_number",
                "numeric",
                "to_number(text, format)",
                "Convert string to number",
            ),
            // === UUID Functions ===
            (
                "gen_random_uuid",
                "uuid",
                "gen_random_uuid()",
                "Generate random UUID v4",
            ),
            (
                "uuid_generate_v4",
                "uuid",
                "uuid_generate_v4()",
                "Generate random UUID v4 (uuid-ossp)",
            ),
            // === System Functions ===
            ("current_user", "name", "current_user", "Current user name"),
            (
                "current_database",
                "name",
                "current_database()",
                "Current database name",
            ),
            (
                "current_schema",
                "name",
                "current_schema()",
                "Current schema name",
            ),
            (
                "current_schemas",
                "name[]",
                "current_schemas(include_implicit)",
                "Current search path",
            ),
            (
                "pg_typeof",
                "regtype",
                "pg_typeof(any)",
                "Get data type of value",
            ),
            ("version", "text", "version()", "PostgreSQL version string"),
            // === Row/Record Functions ===
            (
                "row_to_json",
                "json",
                "row_to_json(record [, pretty])",
                "Convert row to JSON",
            ),
            (
                "row_number",
                "bigint",
                "row_number() OVER (...)",
                "Number of current row",
            ),
            ("rank", "bigint", "rank() OVER (...)", "Rank with gaps"),
            (
                "dense_rank",
                "bigint",
                "dense_rank() OVER (...)",
                "Rank without gaps",
            ),
            ("ntile", "integer", "ntile(n) OVER (...)", "Bucket number"),
            (
                "lag",
                "any",
                "lag(value [, offset [, default]]) OVER (...)",
                "Value from previous row",
            ),
            (
                "lead",
                "any",
                "lead(value [, offset [, default]]) OVER (...)",
                "Value from following row",
            ),
            (
                "first_value",
                "any",
                "first_value(value) OVER (...)",
                "First value in window",
            ),
            (
                "last_value",
                "any",
                "last_value(value) OVER (...)",
                "Last value in window",
            ),
            (
                "nth_value",
                "any",
                "nth_value(value, n) OVER (...)",
                "Nth value in window",
            ),
        ];

        for (name, return_type, signature, description) in FUNCTIONS {
            self.functions.insert(
                name.to_string(),
                FunctionInfo {
                    name: name.to_string(),
                    return_type: return_type.to_string(),
                    signature: signature.to_string(),
                    description: description.to_string(),
                },
            );
        }
    }

    /// Refresh function information from pg_catalog.
    async fn refresh_functions(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String, String, String, String, Option<String>)>(
            r#"
            SELECT
                n.nspname AS schema_name,
                p.proname AS function_name,
                pg_get_function_result(p.oid) AS return_type,
                COALESCE(pg_get_function_identity_arguments(p.oid), '') AS args,
                obj_description(p.oid, 'pg_proc') AS description
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema')
            ORDER BY n.nspname, p.proname
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.functions.clear();

        for (schema, name, return_type, args, description) in rows {
            let args = args.trim();
            let signature = if args.is_empty() {
                format!("{}()", name)
            } else {
                format!("{}({})", name, args)
            };

            let info = FunctionInfo {
                name: name.clone(),
                return_type,
                signature: format!("{}.{}", schema, signature),
                description: description.unwrap_or_default(),
            };

            self.functions.insert(format!("{}.{}", schema, name), info);
        }

        tracing::info!(
            total = self.functions.len(),
            "LSP schema cache: loaded functions"
        );
        Ok(())
    }

    /// Detect if a table name corresponds to a FHIR resource.
    fn detect_fhir_table(table_name: &str) -> (bool, Option<String>) {
        // Common FHIR resource types (in PascalCase, lowercase, or snake_case)
        const FHIR_RESOURCES: &[&str] = &[
            "Patient",
            "Practitioner",
            "Organization",
            "Encounter",
            "Observation",
            "Condition",
            "Procedure",
            "Medication",
            "MedicationRequest",
            "MedicationAdministration",
            "MedicationDispense",
            "MedicationStatement",
            "AllergyIntolerance",
            "Immunization",
            "DiagnosticReport",
            "CarePlan",
            "CareTeam",
            "Goal",
            "ServiceRequest",
            "Appointment",
            "Schedule",
            "Slot",
            "Device",
            "DeviceRequest",
            "DeviceUseStatement",
            "Location",
            "Specimen",
            "ImagingStudy",
            "Coverage",
            "Claim",
            "ClaimResponse",
            "ExplanationOfBenefit",
            "DocumentReference",
            "Binary",
            "Bundle",
            "Composition",
            "Consent",
            "Contract",
            "DetectedIssue",
            "FamilyMemberHistory",
            "Flag",
            "Group",
            "HealthcareService",
            "Invoice",
            "List",
            "MeasureReport",
            "NutritionOrder",
            "OperationOutcome",
            "Person",
            "Provenance",
            "Questionnaire",
            "QuestionnaireResponse",
            "RelatedPerson",
            "RequestGroup",
            "RiskAssessment",
            "SupplyDelivery",
            "SupplyRequest",
            "Task",
        ];

        // Normalize table name to PascalCase for comparison
        let normalized = Self::to_pascal_case(table_name);

        for resource in FHIR_RESOURCES {
            if normalized.eq_ignore_ascii_case(resource) {
                return (true, Some(resource.to_string()));
            }
        }

        (false, None)
    }

    /// Convert a table name to PascalCase.
    fn to_pascal_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;

        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }

        result
    }

    /// Get all schemas.
    pub fn get_schemas(&self) -> Vec<SchemaInfo> {
        self.schemas.iter().map(|r| r.value().clone()).collect()
    }

    /// Get user schemas only (excludes pg_catalog, information_schema, etc.).
    pub fn get_user_schemas(&self) -> Vec<SchemaInfo> {
        self.schemas
            .iter()
            .filter(|r| r.value().is_user_schema)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Check if a schema exists.
    pub fn has_schema(&self, name: &str) -> bool {
        self.schemas.contains_key(name)
    }

    /// Get tables in a specific schema.
    pub fn get_tables_in_schema(&self, schema: &str) -> Vec<TableInfo> {
        self.tables
            .iter()
            .filter(|r| r.value().schema == schema)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get all tables.
    pub fn get_tables(&self) -> Vec<TableInfo> {
        self.tables.iter().map(|r| r.value().clone()).collect()
    }

    /// Get tables matching a prefix.
    pub fn get_tables_matching(&self, prefix: &str) -> Vec<TableInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.tables
            .iter()
            .filter(|r| r.value().name.to_lowercase().starts_with(&prefix_lower))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get a table by exact name (case-insensitive).
    pub fn get_table_by_name(&self, name: &str) -> Option<TableInfo> {
        let name_lower = name.to_lowercase();
        self.tables
            .iter()
            .find(|r| r.value().name.to_lowercase() == name_lower)
            .map(|r| r.value().clone())
    }

    /// Get FHIR resource tables.
    pub fn get_fhir_tables(&self) -> Vec<TableInfo> {
        self.tables
            .iter()
            .filter(|r| r.value().is_fhir_table)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get columns for a table.
    pub fn get_columns(&self, table_name: &str) -> Vec<ColumnInfo> {
        if let Some((schema, table)) = Self::split_qualified_table(table_name) {
            return self.get_columns_in_schema(schema, table);
        }

        if let Some(columns) = self.columns.get(&Self::column_key("public", table_name)) {
            return columns.value().clone();
        }

        let table_lower = table_name.to_lowercase();
        let mut matches = Vec::new();

        for entry in self.columns.iter() {
            if entry
                .value()
                .first()
                .is_some_and(|column| column.table_name.to_lowercase() == table_lower)
            {
                matches.push(entry.value().clone());
            }
        }

        if matches.len() == 1 {
            matches.pop().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Get columns for a table within a specific schema.
    pub fn get_columns_in_schema(&self, schema: &str, table_name: &str) -> Vec<ColumnInfo> {
        self.columns
            .get(&Self::column_key(schema, table_name))
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    /// Get columns matching a prefix for a table.
    pub fn get_columns_matching(&self, table_name: &str, prefix: &str) -> Vec<ColumnInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.get_columns(table_name)
            .into_iter()
            .filter(|c| c.name.to_lowercase().starts_with(&prefix_lower))
            .collect()
    }

    /// Get JSONB columns for a table.
    pub fn get_jsonb_columns(&self, table_name: &str) -> Vec<ColumnInfo> {
        self.get_columns(table_name)
            .into_iter()
            .filter(|c| c.data_type == "jsonb")
            .collect()
    }

    /// Get all JSONB functions.
    pub fn get_functions(&self) -> Vec<FunctionInfo> {
        self.functions.iter().map(|r| r.value().clone()).collect()
    }

    /// Get functions matching a prefix.
    pub fn get_functions_matching(&self, prefix: &str) -> Vec<FunctionInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.functions
            .iter()
            .filter(|r| r.value().name.to_lowercase().starts_with(&prefix_lower))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get a specific function by name.
    pub fn get_function(&self, name: &str) -> Option<FunctionInfo> {
        self.functions.get(name).map(|r| r.value().clone())
    }

    /// Check if a table exists (case-insensitive).
    pub fn table_exists(&self, name: &str) -> bool {
        self.get_table_by_name(name).is_some()
    }

    /// Check if a function exists (case-insensitive).
    pub fn function_exists(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.functions
            .iter()
            .any(|r| r.value().name.to_lowercase() == name_lower)
    }

    /// Check if a table is a FHIR resource table.
    pub fn is_fhir_table(&self, table_name: &str) -> bool {
        let (schema, table_name) = Self::split_qualified_table(table_name)
            .map(|(schema, table)| (Some(schema), table))
            .unwrap_or((None, table_name));

        self.tables.iter().any(|r| {
            r.value().name == table_name
                && r.value().is_fhir_table
                && schema.is_none_or(|s| r.value().schema == s)
        })
    }

    /// Get the FHIR resource type for a table.
    pub fn get_fhir_resource_type(&self, table_name: &str) -> Option<String> {
        let (schema, table_name) = Self::split_qualified_table(table_name)
            .map(|(schema, table)| (Some(schema), table))
            .unwrap_or((None, table_name));

        self.tables.iter().find_map(|r| {
            if r.value().name == table_name && schema.is_none_or(|s| r.value().schema == s) {
                r.value().fhir_resource_type.clone()
            } else {
                None
            }
        })
    }

    fn column_key(schema: &str, table: &str) -> String {
        format!("{}.{}", schema, table)
    }

    fn split_qualified_table(table_name: &str) -> Option<(&str, &str)> {
        let (schema, table) = table_name.split_once('.')?;

        if schema.is_empty() || table.is_empty() {
            return None;
        }
        Some((schema, table))
    }

    // === Policy methods ===

    /// Get all policies.
    pub fn get_policies(&self) -> Vec<PolicyInfo> {
        self.policies.iter().map(|r| r.value().clone()).collect()
    }

    /// Get policies for a specific table.
    pub fn get_policies_for_table(&self, table_name: &str) -> Vec<PolicyInfo> {
        self.policies
            .iter()
            .filter(|r| r.value().table_name == table_name)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get a policy by name and table.
    pub fn get_policy(&self, table_name: &str, policy_name: &str) -> Option<PolicyInfo> {
        self.policies
            .iter()
            .find(|r| r.value().table_name == table_name && r.value().name == policy_name)
            .map(|r| r.value().clone())
    }

    // === Role methods ===

    /// Get all roles.
    pub fn get_roles(&self) -> Vec<RoleInfo> {
        self.roles.iter().map(|r| r.value().clone()).collect()
    }

    /// Get a role by name.
    pub fn get_role(&self, name: &str) -> Option<RoleInfo> {
        self.roles.get(name).map(|r| r.value().clone())
    }

    /// Get roles that can log in.
    pub fn get_login_roles(&self) -> Vec<RoleInfo> {
        self.roles
            .iter()
            .filter(|r| r.value().can_login)
            .map(|r| r.value().clone())
            .collect()
    }

    // === Type methods ===

    /// Get all custom types.
    pub fn get_types(&self) -> Vec<TypeInfo> {
        self.types.iter().map(|r| r.value().clone()).collect()
    }

    /// Get types in a specific schema.
    pub fn get_types_in_schema(&self, schema: &str) -> Vec<TypeInfo> {
        self.types
            .iter()
            .filter(|r| r.value().schema == schema)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get a type by schema and name.
    pub fn get_type(&self, schema: &str, name: &str) -> Option<TypeInfo> {
        let key = format!("{}.{}", schema, name);
        self.types.get(&key).map(|r| r.value().clone())
    }

    /// Get a type by name (searches all schemas).
    pub fn get_type_by_name(&self, name: &str) -> Option<TypeInfo> {
        self.types
            .iter()
            .find(|r| r.value().name == name)
            .map(|r| r.value().clone())
    }

    /// Get enum types only.
    pub fn get_enum_types(&self) -> Vec<TypeInfo> {
        self.types
            .iter()
            .filter(|r| r.value().category == "Enum")
            .map(|r| r.value().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(SchemaCache::to_pascal_case("patient"), "Patient");
        assert_eq!(SchemaCache::to_pascal_case("Patient"), "Patient");
        // Note: to_pascal_case capitalizes first letter of each segment (split by _ or -)
        // but the FHIR table detection uses case-insensitive comparison
        assert_eq!(
            SchemaCache::to_pascal_case("medication_request"),
            "MedicationRequest"
        );
        assert_eq!(
            SchemaCache::to_pascal_case("allergy_intolerance"),
            "AllergyIntolerance"
        );
    }

    #[test]
    fn test_detect_fhir_table() {
        assert_eq!(
            SchemaCache::detect_fhir_table("patient"),
            (true, Some("Patient".to_string()))
        );
        assert_eq!(
            SchemaCache::detect_fhir_table("Patient"),
            (true, Some("Patient".to_string()))
        );
        assert_eq!(
            SchemaCache::detect_fhir_table("observation"),
            (true, Some("Observation".to_string()))
        );
        // With underscore - should still match via case-insensitive comparison
        assert_eq!(
            SchemaCache::detect_fhir_table("medication_request"),
            (true, Some("MedicationRequest".to_string()))
        );
        assert_eq!(
            SchemaCache::detect_fhir_table("some_random_table"),
            (false, None)
        );
    }
}
