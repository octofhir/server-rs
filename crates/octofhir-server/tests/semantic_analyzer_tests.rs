use octofhir_server::lsp::semantic_analyzer::SemanticAnalyzer;
use sqlparser::ast::BinaryOperator;

// ============================================================================
// JSONB OPERATOR TESTS (->>, ->, #>, #>>)
// ============================================================================

#[test]
fn test_jsonb_arrow() {
    let sql = "SELECT resource -> 'name' FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let op_info = ctx.jsonb_operator.unwrap();
    assert!(matches!(op_info.operator, Some(BinaryOperator::Arrow)));
    assert!(op_info.is_operator());
    assert!(!op_info.is_function());
    assert_eq!(op_info.target(), "resource");
    assert!(op_info.path().is_some());
}

#[test]
fn test_jsonb_long_arrow() {
    let sql = "SELECT resource ->> 'name' FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let op_info = ctx.jsonb_operator.unwrap();
    assert!(matches!(op_info.operator, Some(BinaryOperator::LongArrow)));
    assert_eq!(op_info.target(), "resource");
}

#[test]
fn test_jsonb_hash_arrow() {
    let sql = "SELECT resource #> '{name}' FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let op_info = ctx.jsonb_operator.unwrap();
    assert!(matches!(
        op_info.operator,
        Some(BinaryOperator::HashArrow)
    ));
    assert_eq!(op_info.target(), "resource");
}

#[test]
fn test_jsonb_hash_long_arrow() {
    let sql = "SELECT resource #>> '{name}' FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let op_info = ctx.jsonb_operator.unwrap();
    assert!(matches!(
        op_info.operator,
        Some(BinaryOperator::HashLongArrow)
    ));
    assert_eq!(op_info.target(), "resource");
}

#[test]
fn test_jsonb_with_spaces() {
    let sql = "SELECT resource #>> '{name}' FROM patient";
    assert!(SemanticAnalyzer::analyze(sql).unwrap().jsonb_operator.is_some());
}

#[test]
fn test_jsonb_in_where_clause() {
    let sql = "SELECT * FROM patient WHERE resource -> 'active' = 'true'";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let op_info = ctx.jsonb_operator.unwrap();
    assert!(matches!(op_info.operator, Some(BinaryOperator::Arrow)));
}

#[test]
fn test_jsonb_nested_path() {
    let sql = "SELECT resource -> 'name' -> 'given' FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
}

#[test]
fn test_no_jsonb() {
    let sql = "SELECT id FROM patient";
    assert!(SemanticAnalyzer::analyze(sql).unwrap().jsonb_operator.is_none());
}

// ============================================================================
// JSONB FUNCTION TESTS (jsonb_path_exists, jsonb_extract_path, etc.)
// ============================================================================

#[test]
fn test_jsonb_path_exists() {
    let sql = "SELECT jsonb_path_exists(resource, '$.name') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert!(func_info.is_function());
    assert!(!func_info.is_operator());
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_path_exists");
    assert_eq!(func_info.target(), "resource");
    assert_eq!(func_info.path().unwrap(), "'$.name'");
}

#[test]
fn test_jsonb_path_query() {
    let sql = "SELECT jsonb_path_query(resource, '$.name') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_path_query");
    assert_eq!(func_info.target(), "resource");
}

#[test]
fn test_jsonb_extract_path() {
    let sql = "SELECT jsonb_extract_path(resource, 'name', 'given') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_extract_path");
    assert_eq!(func_info.target(), "resource");
}

#[test]
fn test_jsonb_extract_path_text() {
    let sql = "SELECT jsonb_extract_path_text(resource, 'name') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_extract_path_text");
}

#[test]
fn test_jsonb_array_elements() {
    let sql = "SELECT jsonb_array_elements(resource) FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_array_elements");
    assert_eq!(func_info.target(), "resource");
}

#[test]
fn test_jsonb_each() {
    let sql = "SELECT jsonb_each(resource) FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_each");
}

#[test]
fn test_jsonb_object_keys() {
    let sql = "SELECT jsonb_object_keys(resource) FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_object_keys");
}

#[test]
fn test_jsonb_typeof() {
    let sql = "SELECT jsonb_typeof(resource -> 'name') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    // Should find the arrow operator first (or the function, depending on traversal order)
    assert!(ctx.jsonb_operator.is_some());
}

#[test]
fn test_jsonb_function_in_where() {
    let sql = "SELECT * FROM patient WHERE jsonb_path_exists(resource, '$.active')";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_path_exists");
}

#[test]
fn test_jsonb_path_query_array() {
    let sql = "SELECT jsonb_path_query_array(resource, '$.name[*]') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_path_query_array");
}

#[test]
fn test_jsonb_path_query_first() {
    let sql = "SELECT jsonb_path_query_first(resource, '$.name') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert_eq!(func_info.function_name.as_ref().unwrap(), "jsonb_path_query_first");
}

// ============================================================================
// COMBINED AND EDGE CASE TESTS
// ============================================================================

#[test]
fn test_jsonb_operator_and_function_mixed() {
    // If both operator and function are present, should find at least one
    let sql = "SELECT resource -> 'name', jsonb_path_exists(resource, '$.id') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
}

#[test]
fn test_jsonb_in_case_expression() {
    let sql = "SELECT CASE WHEN resource -> 'active' = 'true' THEN 1 ELSE 0 END FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
}

#[test]
fn test_jsonb_in_between() {
    let sql = "SELECT * FROM patient WHERE (resource ->> 'age')::int BETWEEN 18 AND 65";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
}

#[test]
fn test_no_jsonb_with_regular_function() {
    let sql = "SELECT upper(name) FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_none());
}

#[test]
fn test_jsonb_complex_path() {
    let sql = "SELECT jsonb_path_exists(resource, '$.name[*] ? (@ like_regex \"^J\")') FROM patient";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    let func_info = ctx.jsonb_operator.unwrap();
    assert!(func_info.path().is_some());
}

// ============================================================================
// CLAUSE EXTRACTION TESTS (WHERE, GROUP BY, HAVING, ORDER BY, LIMIT, OFFSET)
// ============================================================================

#[test]
fn test_where_clause() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("WHERE"));
    assert!(!ctx.existing_clauses.contains("GROUP BY"));
    assert!(!ctx.existing_clauses.contains("ORDER BY"));
}

#[test]
fn test_multiple_clauses() {
    let sql = "SELECT * FROM users WHERE id = 1 GROUP BY name ORDER BY created_at";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("WHERE"));
    assert!(ctx.existing_clauses.contains("GROUP BY"));
    assert!(ctx.existing_clauses.contains("ORDER BY"));
}

#[test]
fn test_having() {
    let sql = "SELECT count(*) FROM users GROUP BY name HAVING count(*) > 1";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("GROUP BY"));
    assert!(ctx.existing_clauses.contains("HAVING"));
}

#[test]
fn test_limit_offset() {
    let sql = "SELECT * FROM users LIMIT 10 OFFSET 20";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("LIMIT"));
    assert!(ctx.existing_clauses.contains("OFFSET"));
}

#[test]
fn test_limit_only() {
    let sql = "SELECT * FROM users LIMIT 10";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("LIMIT"));
    assert!(!ctx.existing_clauses.contains("OFFSET"));
}

#[test]
fn test_offset_only() {
    let sql = "SELECT * FROM users OFFSET 5";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(!ctx.existing_clauses.contains("LIMIT"));
    assert!(ctx.existing_clauses.contains("OFFSET"));
}

#[test]
fn test_distinct() {
    let sql = "SELECT DISTINCT name FROM users";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("DISTINCT"));
}

#[test]
fn test_all_clauses() {
    let sql = "SELECT DISTINCT name FROM users WHERE id > 0 GROUP BY name HAVING count(*) > 1 ORDER BY name LIMIT 10 OFFSET 5";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.contains("DISTINCT"));
    assert!(ctx.existing_clauses.contains("WHERE"));
    assert!(ctx.existing_clauses.contains("GROUP BY"));
    assert!(ctx.existing_clauses.contains("HAVING"));
    assert!(ctx.existing_clauses.contains("ORDER BY"));
    assert!(ctx.existing_clauses.contains("LIMIT"));
    assert!(ctx.existing_clauses.contains("OFFSET"));
}

#[test]
fn test_no_clauses() {
    let sql = "SELECT * FROM users";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.existing_clauses.is_empty());
}
