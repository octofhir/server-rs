//! Comprehensive tests for ALL PostgreSQL syntax support
//!
//! These tests use insta snapshot testing to verify that the formatter correctly
//! handles the full range of PostgreSQL syntax via pg_query.
//!
//! All formatting follows sqlstyle.guide rules (hardcoded, no configuration).
//!
//! To review snapshots: cargo insta review
//! To accept all snapshots: cargo insta accept

use crate::lsp::formatting::SqlFormatter;

/// Helper to create a formatter with sqlstyle.guide rules
fn default_formatter() -> SqlFormatter {
    SqlFormatter::new()
}

#[test]
fn test_basic_select() {
    let formatter = default_formatter();
    let sql = "SELECT id, name FROM patient";
    let formatted = formatter.format(sql).unwrap();

    // Snapshot the formatted output
    insta::assert_snapshot!(formatted);

    // Verify zero-loss guarantee
    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_select_with_where() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient WHERE active = true";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_select_with_multiple_conditions() {
    let formatter = default_formatter();
    let sql = "SELECT id, name, status FROM patient WHERE active = true AND status = 'active' OR deleted = false";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_select_with_order_by() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient ORDER BY name ASC, id DESC";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_select_with_group_by_having() {
    let formatter = default_formatter();
    let sql = "SELECT status, COUNT(*) FROM patient GROUP BY status HAVING COUNT(*) > 5";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_select_with_limit_offset() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient LIMIT 10 OFFSET 5";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_inner_join() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient INNER JOIN observation ON patient.id = observation.subject";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_left_join() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient LEFT JOIN observation ON patient.id = observation.subject";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_multiple_joins() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient p LEFT JOIN observation o ON p.id = o.subject LEFT JOIN practitioner pr ON o.practitioner = pr.id";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_insert_single_row() {
    let formatter = default_formatter();
    let sql = "INSERT INTO patient (id, name) VALUES (1, 'John')";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_insert_multiple_rows() {
    let formatter = default_formatter();
    let sql = "INSERT INTO patient (id, name) VALUES (1, 'John'), (2, 'Jane'), (3, 'Bob')";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_update_statement() {
    let formatter = default_formatter();
    let sql = "UPDATE patient SET name = 'Jane Doe' WHERE id = 1";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_delete_statement() {
    let formatter = default_formatter();
    let sql = "DELETE FROM patient WHERE id = 1";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_create_table() {
    let formatter = default_formatter();
    let sql = "CREATE TABLE test (id INT, name TEXT)";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_create_table_with_constraints() {
    let formatter = default_formatter();
    let sql = "CREATE TABLE patient (id INT PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE)";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_drop_table() {
    let formatter = default_formatter();
    let sql = "DROP TABLE IF EXISTS test CASCADE";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_simple_cte() {
    let formatter = default_formatter();
    let sql = "WITH cte AS (SELECT * FROM patient) SELECT * FROM cte";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_cte_with_where() {
    let formatter = default_formatter();
    let sql = "WITH active_patients AS (SELECT * FROM patient WHERE active = true) SELECT * FROM active_patients";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_multiple_ctes() {
    let formatter = default_formatter();
    let sql = "WITH cte1 AS (SELECT * FROM patient), cte2 AS (SELECT * FROM observation) SELECT * FROM cte1 INNER JOIN cte2 ON cte1.id = cte2.subject";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_window_function_row_number() {
    let formatter = default_formatter();
    let sql = "SELECT id, name, ROW_NUMBER() OVER (ORDER BY created_at) FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_window_function_partition_by() {
    let formatter = default_formatter();
    let sql = "SELECT id, name, status, ROW_NUMBER() OVER (PARTITION BY status ORDER BY created_at) FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_subquery_in_where() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient WHERE id IN (SELECT patient_id FROM observation)";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_subquery_with_exists() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient p WHERE EXISTS (SELECT 1 FROM observation o WHERE o.subject = p.id)";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_union() {
    let formatter = default_formatter();
    let sql = "SELECT id FROM patient UNION SELECT id FROM practitioner";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_intersect() {
    let formatter = default_formatter();
    let sql = "SELECT id FROM patient INTERSECT SELECT patient_id FROM observation";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_case_expression() {
    let formatter = default_formatter();
    let sql = "SELECT id, CASE WHEN active THEN 'Active' ELSE 'Inactive' END FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_complex_real_world_query() {
    let formatter = default_formatter();
    let sql = r#"
        SELECT
            p.id,
            p.resource->>'name' as patient_name,
            COUNT(o.id) as observation_count
        FROM patient p
        LEFT JOIN observation o ON p.id = o.resource->>'subject'
        WHERE p.resource @> '{"active": true}'
            AND o.resource->>'status' = 'final'
        GROUP BY p.id, p.resource->>'name'
        HAVING COUNT(o.id) > 5
        ORDER BY observation_count DESC
        LIMIT 100
    "#;
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

// ============================================================================
// PostgreSQL Extension Tests
// ============================================================================

#[test]
fn test_jsonb_arrow_operator() {
    let formatter = default_formatter();
    let sql = "SELECT resource->'name' FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_jsonb_double_arrow_operator() {
    let formatter = default_formatter();
    let sql = "SELECT resource->>'given' FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_jsonb_path_operators() {
    let formatter = default_formatter();
    let sql = "SELECT data#>'{a,b}', data#>>'{x,y}' FROM test";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_jsonb_contains_operator() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient WHERE resource@>'{\"active\":true}'";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_jsonb_contained_by_operator() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM patient WHERE '{\"status\":\"active\"}'<@resource";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_jsonb_question_operators() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM test WHERE data?'key' OR data?|ARRAY['a','b'] OR data?&ARRAY['x','y']";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_array_overlap_operator() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM test WHERE tags&&ARRAY['urgent','priority']";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_array_constructor() {
    let formatter = default_formatter();
    let sql = "SELECT ARRAY[1,2,3,4,5] as numbers";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_any_all_operators() {
    let formatter = default_formatter();
    let sql = "SELECT * FROM test WHERE id=ANY(ARRAY[1,2,3]) AND status!=ALL(ARRAY['deleted','archived'])";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_string_concatenation() {
    let formatter = default_formatter();
    let sql = "SELECT first_name||' '||last_name as full_name FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_complex_jsonb_chaining() {
    let formatter = default_formatter();
    let sql = "SELECT resource->'name'->'family', resource->'name'->0->>'given' FROM patient";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}

#[test]
fn test_join_with_and_in_on_clause() {
    let formatter = default_formatter();
    let sql = "SELECT r.last_name FROM riders r INNER JOIN bikes b ON r.bike_vin_num=b.vin_num AND b.engine_tally>2 INNER JOIN crew c ON r.crew_chief_last_name=c.last_name AND c.chief='Y'";
    let formatted = formatter.format(sql).unwrap();

    insta::assert_snapshot!(formatted);

    let original_fp = pg_query::fingerprint(sql).unwrap();
    let formatted_fp = pg_query::fingerprint(&formatted).unwrap();
    assert_eq!(original_fp.value, formatted_fp.value);
}
