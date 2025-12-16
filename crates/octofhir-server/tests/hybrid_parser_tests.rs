use octofhir_server::lsp::semantic_analyzer::SemanticAnalyzer;

#[test]
fn test_complete_sql_with_jsonb_and_clauses() {
    let sql = "SELECT resource #>> '{name}' FROM patient WHERE id = 1";
    let ctx = SemanticAnalyzer::analyze(sql).unwrap();

    assert!(ctx.jsonb_operator.is_some());
    assert!(ctx.existing_clauses.contains("WHERE"));
}

#[test]
fn test_incomplete_sql_fails_gracefully() {
    let sql = "SELECT resource #>> '{name}' FROM patient WHERE";
    // sqlparser-rs should fail on incomplete SQL
    assert!(SemanticAnalyzer::analyze(sql).is_none());
}

#[test]
fn test_complex_query() {
    let sql = r#"
        SELECT
            resource #>> '{name,given}' as given_name
        FROM patient
        WHERE resource @> '{"active": true}'
        GROUP BY resource #>> '{name,family}'
        HAVING count(*) > 1
        ORDER BY given_name
        LIMIT 100
    "#;

    let ctx = SemanticAnalyzer::analyze(sql).unwrap();
    assert!(ctx.jsonb_operator.is_some());
    assert!(ctx.existing_clauses.contains("WHERE"));
    assert!(ctx.existing_clauses.contains("GROUP BY"));
    assert!(ctx.existing_clauses.contains("HAVING"));
    assert!(ctx.existing_clauses.contains("ORDER BY"));
    assert!(ctx.existing_clauses.contains("LIMIT"));
}

#[test]
fn test_various_spacing() {
    let cases = vec![
        "SELECT resource#>>'{name}' FROM patient",
        "SELECT resource #>> '{name}' FROM patient",
        "SELECT resource  #>>  '{name}' FROM patient",
        "SELECT resource->'name' FROM patient",
        "SELECT resource -> 'name' FROM patient",
    ];

    for sql in cases {
        let ctx = SemanticAnalyzer::analyze(sql);
        assert!(ctx.is_some(), "Failed: {}", sql);
        assert!(ctx.unwrap().jsonb_operator.is_some());
    }
}
