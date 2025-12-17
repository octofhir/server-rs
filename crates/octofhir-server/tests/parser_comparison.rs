/// Proof of Concept: Compare tree-sitter vs sqlparser-rs for JSONB operator detection
///
/// This test evaluates both parsers to determine which is better suited for SQL completion
/// with focus on JSONB operators and space handling.

#[cfg(test)]
mod parser_comparison {
    use sqlparser::ast::{BinaryOperator, Expr, Statement};
    use sqlparser::dialect::PostgreSqlDialect;
    use sqlparser::parser::Parser;

    /// Test cases for JSONB operator detection
    const TEST_CASES: &[(&str, &str)] = &[
        ("SELECT resource#>>'{path}' FROM users", "no spaces"),
        (
            "SELECT resource #>> '{path}' FROM users",
            "spaces before operator",
        ),
        ("SELECT resource -> 'name' FROM users", "spaces around ->"),
        (
            "SELECT resource->'name'->>'family' FROM users",
            "chained operators",
        ),
        (
            "SELECT resource #> '{name,given}' FROM users",
            "array path with spaces",
        ),
        (
            "SELECT * FROM users WHERE resource->>'status' = 'active'",
            "JSONB in WHERE clause",
        ),
        (
            "SELECT resource->'name' AS patient_name FROM users",
            "JSONB with alias",
        ),
    ];

    #[test]
    fn test_sqlparser_rs_jsonb_detection() {
        println!("\n=== Testing sqlparser-rs ===\n");

        let dialect = PostgreSqlDialect {};

        for (sql, description) in TEST_CASES {
            println!("Test: {} ({})", sql, description);

            let parse_result = Parser::parse_sql(&dialect, sql);

            match parse_result {
                Ok(statements) => {
                    println!("  ✅ Parsed successfully");
                    println!("  Statements: {}", statements.len());

                    // Analyze first statement (SELECT)
                    if let Some(Statement::Query(query)) = statements.first() {
                        if let sqlparser::ast::SetExpr::Select(select) = &*query.body {
                            for (i, projection) in select.projection.iter().enumerate() {
                                println!("  Column {}: {:?}", i, projection);

                                // Check for JSONB operators in expressions
                                if let sqlparser::ast::SelectItem::UnnamedExpr(expr) = projection {
                                    analyze_jsonb_expr(expr, 0);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("  ❌ Parse error: {}", e);
                }
            }
            println!();
        }
    }

    /// Recursively analyze expressions for JSONB operators
    fn analyze_jsonb_expr(expr: &Expr, depth: usize) {
        let indent = "  ".repeat(depth + 2);

        match expr {
            Expr::BinaryOp { left, op, right } => {
                let is_jsonb = matches!(
                    op,
                    BinaryOperator::Arrow
                        | BinaryOperator::LongArrow
                        | BinaryOperator::HashArrow
                        | BinaryOperator::HashLongArrow
                );

                if is_jsonb {
                    println!("{}🎯 JSONB operator detected: {:?}", indent, op);
                    println!("{}   Left: {:?}", indent, left);
                    println!("{}   Right: {:?}", indent, right);
                }

                // Recurse into nested expressions (for chained operators)
                analyze_jsonb_expr(left, depth + 1);
                analyze_jsonb_expr(right, depth + 1);
            }
            Expr::Identifier(ident) => {
                println!("{}Identifier: {}", indent, ident);
            }
            Expr::Value(val) => {
                println!("{}Value: {:?}", indent, val);
            }
            _ => {}
        }
    }

    #[test]
    fn test_tree_sitter_jsonb_detection() {
        println!("\n=== Testing tree-sitter ===\n");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load tree-sitter grammar");

        for (sql, description) in TEST_CASES {
            println!("Test: {} ({})", sql, description);

            let tree = parser.parse(sql, None);

            match tree {
                Some(tree) => {
                    println!("  ✅ Parsed successfully");

                    let root = tree.root_node();
                    find_jsonb_operators(&root, sql.as_bytes(), 0);
                }
                None => {
                    println!("  ❌ Parse failed");
                }
            }
            println!();
        }
    }

    /// Recursively find JSONB operators in tree-sitter AST
    fn find_jsonb_operators(node: &tree_sitter::Node, source: &[u8], depth: usize) {
        let indent = "  ".repeat(depth + 1);

        // Check if this is a binary expression with JSONB operator
        if node.kind() == "binary_expression" {
            // Look for op_other child (JSONB operators)
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    if child.kind() == "op_other" {
                        if let Ok(op_text) = child.utf8_text(source) {
                            if matches!(op_text, "->" | "->>" | "#>" | "#>>") {
                                println!("{}🎯 JSONB operator: {}", indent, op_text);

                                // Get left and right operands
                                if let Some(left) = node.child_by_field_name("binary_expr_left") {
                                    if let Ok(left_text) = left.utf8_text(source) {
                                        println!("{}   Left: {}", indent, left_text);
                                    }
                                }
                                if let Some(right) = node.child_by_field_name("binary_expr_right") {
                                    if let Ok(right_text) = right.utf8_text(source) {
                                        println!("{}   Right: {}", indent, right_text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recurse into children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                find_jsonb_operators(&child, source, depth + 1);
            }
        }
    }

    #[test]
    fn benchmark_comparison() {
        use std::time::Instant;

        println!("\n=== Performance Comparison ===\n");

        let sql = "SELECT resource->'name'->>'family', resource->>'status' FROM users WHERE resource#>>'{active}' = 'true'";

        // Benchmark sqlparser-rs
        let start = Instant::now();
        let dialect = PostgreSqlDialect {};
        for _ in 0..1000 {
            let _ = Parser::parse_sql(&dialect, sql);
        }
        let sqlparser_duration = start.elapsed();
        println!("sqlparser-rs: 1000 parses in {:?}", sqlparser_duration);
        println!("  Average: {:?} per parse", sqlparser_duration / 1000);

        // Benchmark tree-sitter
        let start = Instant::now();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        for _ in 0..1000 {
            let _ = parser.parse(sql, None);
        }
        let tree_sitter_duration = start.elapsed();
        println!("\ntree-sitter: 1000 parses in {:?}", tree_sitter_duration);
        println!("  Average: {:?} per parse", tree_sitter_duration / 1000);

        // Compare
        println!("\n--- Results ---");
        if sqlparser_duration < tree_sitter_duration {
            let speedup =
                tree_sitter_duration.as_nanos() as f64 / sqlparser_duration.as_nanos() as f64;
            println!("✅ sqlparser-rs is {:.2}x FASTER", speedup);
        } else {
            let speedup =
                sqlparser_duration.as_nanos() as f64 / tree_sitter_duration.as_nanos() as f64;
            println!("✅ tree-sitter is {:.2}x FASTER", speedup);
        }
    }

    #[test]
    fn test_error_recovery() {
        println!("\n=== Error Recovery Comparison ===\n");

        // Incomplete SQL (typing in progress)
        let incomplete_cases = &[
            "SELECT resource->",
            "SELECT resource #>>",
            "SELECT resource->'name'->",
            "SELECT * FROM users WHERE resource",
        ];

        println!("--- sqlparser-rs ---");
        let dialect = PostgreSqlDialect {};
        for sql in incomplete_cases {
            let result = Parser::parse_sql(&dialect, sql);
            match result {
                Ok(_) => println!("  ✅ '{}' parsed", sql),
                Err(e) => println!("  ❌ '{}' error: {}", sql, e),
            }
        }

        println!("\n--- tree-sitter ---");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        for sql in incomplete_cases {
            if let Some(tree) = parser.parse(sql, None) {
                let has_error = tree.root_node().has_error();
                if has_error {
                    println!(
                        "  ⚠️  '{}' parsed with errors (still usable for completion)",
                        sql
                    );
                } else {
                    println!("  ✅ '{}' parsed successfully", sql);
                }
            } else {
                println!("  ❌ '{}' failed to parse", sql);
            }
        }
    }
}
