//! SQL Style Guide constants (sqlstyle.guide)
//!
//! This module contains hardcoded formatting rules based on https://www.sqlstyle.guide/
//! All rules are mandatory and cannot be configured.

use std::collections::HashSet;

/// Indentation: Always 4 spaces (never tabs)
/// Reference: sqlstyle.guide - "Use 4 spaces for indentation"
pub const INDENT_SIZE: usize = 4;

/// Never use tabs for indentation
pub const USE_TABS: bool = false;

/// Keywords: Always UPPERCASE
/// Reference: sqlstyle.guide - "Use UPPERCASE for reserved keywords"
pub const KEYWORD_UPPERCASE: bool = true;

/// Identifiers: Always lowercase (unless quoted)
/// Reference: sqlstyle.guide - "Use lowercase for identifiers"
pub const IDENTIFIER_LOWERCASE: bool = true;

/// Maximum line width (0 = no limit)
/// Using 120 as a reasonable modern standard
pub const MAX_LINE_WIDTH: usize = 120;

/// Always use trailing commas
/// Reference: sqlstyle.guide - commas at end of line
pub const TRAILING_COMMAS: bool = true;

/// Add spaces around JSONB operators (PostgreSQL extension)
/// Follows sqlstyle.guide principle of spacing around operators
pub const JSONB_OPERATOR_SPACING: bool = true;

/// Add spaces around comparison operators (=, <, >, etc.)
/// Reference: sqlstyle.guide - "Spaces before and after equals"
pub const COMPARISON_OPERATOR_SPACING: bool = true;

/// Indent JOIN clauses
/// Reference: sqlstyle.guide - JOIN indented to align with table content
pub const INDENT_JOINS: bool = true;

/// Indent WHERE clause conditions
/// Reference: sqlstyle.guide - WHERE conditions indented
pub const INDENT_WHERE: bool = true;

/// Keyword alignment: Right-aligned to create "river"
/// Reference: sqlstyle.guide - keywords aligned to create visual flow
pub const RIGHT_ALIGN_KEYWORDS: bool = true;

/// Keyword padding width for right alignment (in characters)
/// Set to 8 to accommodate "ORDER BY" (longest common multi-word keyword)
pub const KEYWORD_PADDING_WIDTH: usize = 8;

/// JOIN indentation from FROM keyword (in spaces)
/// Reference: sqlstyle.guide example shows 7 spaces
pub const JOIN_INDENT_SPACES: usize = 7;

/// ON clause indentation (same as JOIN)
pub const ON_CLAUSE_INDENT_SAME_AS_JOIN: bool = true;

/// AND/OR indentation within JOIN ON clause (extra spaces beyond JOIN)
/// Reference: sqlstyle.guide example shows 3 extra spaces
pub const JOIN_CONDITION_EXTRA_INDENT: usize = 3;

/// Naming convention limits

/// Maximum identifier length in bytes
/// Reference: sqlstyle.guide - "Limit to 30 bytes"
pub const MAX_IDENTIFIER_LENGTH: usize = 30;

/// Preferred column suffixes for clarity
/// Reference: sqlstyle.guide - "Apply uniform suffixes"
pub const PREFERRED_SUFFIXES: &[&str] = &[
    "_id",     // unique identifier
    "_status", // flag or status value
    "_total",  // sum or total
    "_num",    // any kind of number
    "_name",   // name field
    "_date",   // date field
    "_tally",  // count
    "_size",   // size of something
    "_addr",   // address (abbreviated)
    "_seq",    // sequence number
];

/// Get comprehensive list of PostgreSQL/SQL reserved keywords
/// This list should be comprehensive to avoid using reserved words as identifiers
pub fn get_reserved_keywords() -> HashSet<&'static str> {
    vec![
        // SQL standard keywords
        "SELECT", "FROM", "WHERE", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "CROSS",
        "FULL", "ON", "AND", "OR", "NOT", "IN", "EXISTS", "BETWEEN", "LIKE", "ILIKE",
        "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "OFFSET", "FETCH", "FIRST", "LAST",
        "AS", "WITH", "CASE", "WHEN", "THEN", "ELSE", "END", "NULLIF", "COALESCE",
        "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "RETURNING",
        "CREATE", "ALTER", "DROP", "TABLE", "VIEW", "INDEX", "SEQUENCE", "FUNCTION",
        "PROCEDURE", "TRIGGER", "CONSTRAINT", "SCHEMA", "DATABASE",
        "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "CHECK", "DEFAULT",
        "NULL", "NOT", "DISTINCT", "ALL", "ANY", "SOME", "EXCEPT", "INTERSECT",
        "UNION", "TRUE", "FALSE", "UNKNOWN", "IS", "ISNULL", "NOTNULL",
        "CAST", "ARRAY", "ROW", "OVER", "PARTITION", "WINDOW", "ROWS", "RANGE",
        "PRECEDING", "FOLLOWING", "UNBOUNDED", "CURRENT", "EXCLUDE", "TIES", "ONLY",
        "FOR", "DO", "LOOP", "WHILE", "REPEAT", "UNTIL", "IF", "ELSIF", "RETURN",
        "RAISE", "EXCEPTION", "BEGIN", "COMMIT", "ROLLBACK", "SAVEPOINT",
        "GRANT", "REVOKE", "EXECUTE", "USAGE", "PRIVILEGES", "TO", "PUBLIC",
        // PostgreSQL-specific keywords
        "CONFLICT", "NOTHING", "UPSERT", "EXCLUDED",
        "LATERAL", "ORDINALITY", "TABLESAMPLE", "BERNOULLI", "SYSTEM",
        "CUBE", "ROLLUP", "GROUPING", "SETS",
        "FILTER", "WITHIN", "GROUPS",
        "GENERATED", "ALWAYS", "STORED", "IDENTITY",
        "INHERIT", "TABLESPACE", "OWNER", "EXTENSION",
        "MATERIALIZED", "REFRESH", "CONCURRENTLY",
        "VACUUM", "ANALYZE", "CLUSTER", "REINDEX",
        "COPY", "TRUNCATE", "LOCK", "LISTEN", "NOTIFY", "UNLISTEN",
        "PREPARE", "DEALLOCATE", "DISCARD", "EXPLAIN", "LOAD",
        "SECURITY", "INVOKER", "DEFINER", "STRICT", "IMMUTABLE",
        "STABLE", "VOLATILE", "LEAKPROOF", "PARALLEL", "SAFE", "UNSAFE", "RESTRICTED",
        // Common data types (avoid as identifiers)
        "INTEGER", "INT", "SMALLINT", "BIGINT", "DECIMAL", "NUMERIC",
        "REAL", "DOUBLE", "PRECISION", "FLOAT",
        "SERIAL", "BIGSERIAL", "SMALLSERIAL",
        "MONEY", "BOOLEAN", "BOOL",
        "CHAR", "VARCHAR", "CHARACTER", "VARYING", "TEXT",
        "BYTEA", "BIT", "VARBIT",
        "DATE", "TIME", "TIMESTAMP", "TIMESTAMPTZ", "INTERVAL",
        "UUID", "JSON", "JSONB", "XML",
        "POINT", "LINE", "LSEG", "BOX", "PATH", "POLYGON", "CIRCLE",
        "CIDR", "INET", "MACADDR", "MACADDR8",
        "TSVECTOR", "TSQUERY",
        "INT4RANGE", "INT8RANGE", "NUMRANGE", "TSRANGE", "TSTZRANGE", "DATERANGE",
    ]
    .into_iter()
    .collect()
}

/// Get major SQL keywords for alignment purposes
/// These are the keywords that should be right-aligned to create the "river"
pub fn get_major_keywords() -> HashSet<&'static str> {
    vec![
        // Query structure
        "SELECT", "FROM", "WHERE", "JOIN", "ON",
        "LEFT", "RIGHT", "INNER", "OUTER", "CROSS",
        "LEFT JOIN", "RIGHT JOIN", "INNER JOIN", "OUTER JOIN", "CROSS JOIN",
        "FULL", "FULL JOIN", "FULL OUTER JOIN",
        "LEFT OUTER JOIN", "RIGHT OUTER JOIN",
        // Grouping and ordering
        "GROUP BY", "HAVING", "ORDER BY", "LIMIT", "OFFSET",
        // CTEs and subqueries
        "WITH", "AS",
        // Set operations
        "UNION", "UNION ALL", "INTERSECT", "EXCEPT",
        // DML
        "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "RETURNING",
        // DDL (less common in queries but included)
        "CREATE", "ALTER", "DROP", "TABLE", "INDEX", "VIEW",
        // Window functions
        "PARTITION BY", "OVER",
        // Conditional
        "CASE", "WHEN", "THEN", "ELSE", "END",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_constants() {
        assert_eq!(INDENT_SIZE, 4);
        assert_eq!(USE_TABS, false);
        assert_eq!(KEYWORD_UPPERCASE, true);
        assert_eq!(IDENTIFIER_LOWERCASE, true);
        assert_eq!(JOIN_INDENT_SPACES, 7);
        assert_eq!(JOIN_CONDITION_EXTRA_INDENT, 3);
    }

    #[test]
    fn test_reserved_keywords_not_empty() {
        let keywords = get_reserved_keywords();
        assert!(!keywords.is_empty());
        assert!(keywords.contains("SELECT"));
        assert!(keywords.contains("FROM"));
        assert!(keywords.contains("WHERE"));
    }

    #[test]
    fn test_major_keywords_not_empty() {
        let keywords = get_major_keywords();
        assert!(!keywords.is_empty());
        assert!(keywords.contains("SELECT"));
        assert!(keywords.contains("LEFT JOIN"));
        assert!(keywords.contains("ORDER BY"));
    }

    #[test]
    fn test_preferred_suffixes() {
        assert!(PREFERRED_SUFFIXES.contains(&"_id"));
        assert!(PREFERRED_SUFFIXES.contains(&"_status"));
        assert!(PREFERRED_SUFFIXES.contains(&"_date"));
    }

    #[test]
    fn test_max_identifier_length() {
        assert_eq!(MAX_IDENTIFIER_LENGTH, 30);
    }
}
