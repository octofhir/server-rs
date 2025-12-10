/// Test tree-sitter grammar's ability to parse JSONB operators
/// This test investigates if we can use tree-sitter AST for JSONB path completion
/// instead of the current regex-based approach.

#[test]
fn test_treesitter_jsonb_parsing() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
        .expect("Failed to load tree-sitter grammar");

    let test_cases = vec![
        // Complete JSONB expressions
        ("SELECT resource->'name' FROM patient", "Complete single arrow"),
        (
            "SELECT resource->'name'->>'given' FROM patient",
            "Complete double arrow",
        ),
        ("SELECT resource#>'{name,0}' FROM patient", "Hash arrow with path"),
        (
            "SELECT resource#>>'{name,given}' FROM patient",
            "Hash double arrow",
        ),
        // Incomplete expressions (critical for LSP completions!)
        ("SELECT resource->'name'-> FROM patient", "Incomplete after arrow"),
        ("SELECT resource->' FROM patient", "Incomplete string key"),
        ("SELECT resource-> FROM patient", "Incomplete no operand"),
        (
            "SELECT resource->'name'->'given'->",
            "Incomplete chained access",
        ),
        // Edge cases
        ("SELECT resource->'name'->0->'system' FROM patient", "Array index"),
        (
            "SELECT resource @> '{\"name\": \"test\"}' FROM patient",
            "Contains operator",
        ),
    ];

    println!("\n===== Tree-sitter JSONB Parsing Test =====\n");

    for (sql, description) in test_cases {
        println!("Test: {}", description);
        println!("SQL:  {}", sql);

        let tree = parser.parse(sql, None);

        match tree {
            Some(tree) => {
                let root = tree.root_node();
                println!("✓ Parsed successfully");
                println!("  Root: {} ({}..{})", root.kind(), root.start_byte(), root.end_byte());

                // Print AST structure
                print_ast_tree(&root, sql.as_bytes(), 1);

                // Check for errors in the tree
                if root.has_error() {
                    println!("  ⚠️  Tree contains ERROR nodes");
                } else {
                    println!("  ✓ No errors in AST");
                }

                // Try to find JSONB operator nodes
                find_jsonb_operators(&root, sql.as_bytes());
            }
            None => {
                println!("✗ Failed to parse");
            }
        }

        println!("{}\n", "-".repeat(80));
    }
}

/// Recursively print the AST tree structure
fn print_ast_tree(node: &tree_sitter::Node, source: &[u8], depth: usize) {
    let indent = "  ".repeat(depth);
    let text = node.utf8_text(source).unwrap_or("<invalid utf8>");
    let text_preview = if text.len() > 50 {
        format!("{}...", &text[..47])
    } else {
        text.to_string()
    };

    println!(
        "{}├─ {} [{}..{}] \"{}\"",
        indent,
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        text_preview.replace('\n', "\\n")
    );

    // Limit depth to avoid too much output
    if depth < 4 {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                print_ast_tree(&child, source, depth + 1);
            }
        }
    } else if node.child_count() > 0 {
        println!("{}  ... ({} more children)", indent, node.child_count());
    }
}

/// Find JSONB operators in the AST
fn find_jsonb_operators(node: &tree_sitter::Node, source: &[u8]) {
    let jsonb_ops = vec![
        "->", "->>", "#>", "#>>", "@>", "<@", "?", "?|", "?&",
    ];

    let mut found_ops = Vec::new();

    // Recursively search for operator nodes
    search_operators(node, source, &jsonb_ops, &mut found_ops);

    if !found_ops.is_empty() {
        println!("  📍 Found JSONB operators:");
        for (op, node_kind, text) in found_ops {
            println!("    {} (node: {}, text: \"{}\")", op, node_kind, text);
        }
    } else {
        println!("  ⚠️  No JSONB operators found in AST");
    }
}

fn search_operators(
    node: &tree_sitter::Node,
    source: &[u8],
    ops: &[&str],
    found: &mut Vec<(String, String, String)>,
) {
    // Check current node
    let text = node.utf8_text(source).unwrap_or("");
    for op in ops {
        if text == *op {
            found.push((
                op.to_string(),
                node.kind().to_string(),
                text.to_string(),
            ));
        }
    }

    // Check if node kind suggests operator
    let kind = node.kind();
    if kind.contains("operator") || kind.contains("arrow") || kind.contains("op") {
        found.push((
            "?".to_string(),
            kind.to_string(),
            text.to_string(),
        ));
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            search_operators(&child, source, ops, found);
        }
    }
}

/// Test if we can extract path segments from tree-sitter AST
#[test]
fn test_extract_jsonb_path_from_ast() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
        .expect("Failed to load tree-sitter grammar");

    let test_cases = vec![
        (
            "SELECT resource->'name' FROM patient",
            vec!["name"],
            "Single path segment",
        ),
        (
            "SELECT resource->'name'->'given' FROM patient",
            vec!["name", "given"],
            "Chained path segments",
        ),
        (
            "SELECT resource->'name'->0->'value' FROM patient",
            vec!["name", "0", "value"],
            "Path with array index",
        ),
        (
            "SELECT resource->'identifier'->0->'system' FROM patient",
            vec!["identifier", "0", "system"],
            "Complex nested path",
        ),
        (
            "SELECT data->>'code' FROM observation",
            vec!["code"],
            "Double arrow operator",
        ),
    ];

    println!("\n===== Testing JSONB Path Extraction =====\n");

    for (sql, expected_segments, description) in test_cases {
        println!("Test: {}", description);
        println!("SQL:  {}", sql);
        println!("Expected segments: {:?}", expected_segments);

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        // Find JSONB expression in the tree
        if let Some(jsonb_expr) = find_jsonb_expr_in_tree(&root, sql) {
            let segments = extract_segments_from_node(&jsonb_expr, sql);
            println!("Extracted segments: {:?}", segments);

            // Validate extraction
            if segments == expected_segments {
                println!("✓ Extraction successful!");
            } else {
                println!("✗ Extraction mismatch!");
                println!("  Expected: {:?}", expected_segments);
                println!("  Got:      {:?}", segments);
            }
        } else {
            println!("✗ No JSONB expression found");
        }

        println!("{}\n", "-".repeat(80));
    }
}

/// Helper: Find a JSONB expression node in the tree
fn find_jsonb_expr_in_tree<'a>(node: &tree_sitter::Node<'a>, text: &str) -> Option<tree_sitter::Node<'a>> {
    // Check if this node contains JSONB operators
    if contains_jsonb_op(node, text) {
        return Some(*node);
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(found) = find_jsonb_expr_in_tree(&child, text) {
                return Some(found);
            }
        }
    }

    None
}

/// Helper: Check if node contains JSONB operators
fn contains_jsonb_op(node: &tree_sitter::Node, text: &str) -> bool {
    if node.kind() == "op_other" {
        if let Ok(op_text) = node.utf8_text(text.as_bytes()) {
            if matches!(op_text, "->" | "->>" | "#>" | "#>>") {
                return true;
            }
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "op_other" {
                if let Ok(op_text) = child.utf8_text(text.as_bytes()) {
                    if matches!(op_text, "->" | "->>" | "#>" | "#>>") {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Helper: Extract segments from a JSONB expression node
fn extract_segments_from_node(node: &tree_sitter::Node, text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    collect_segments(node, text, &mut segments);
    segments
}

/// Helper: Recursively collect path segments
fn collect_segments(node: &tree_sitter::Node, text: &str, segments: &mut Vec<String>) {
    // String literal (path segment) - check both "string" and "literal"
    if node.kind() == "string" || node.kind() == "literal" {
        if let Ok(segment_text) = node.utf8_text(text.as_bytes()) {
            let cleaned = segment_text.trim_matches('\'').trim_matches('"');
            if !cleaned.is_empty() {
                segments.push(cleaned.to_string());
            }
        }
        return;
    }

    // Number (array index) - check both "number" and "integer"
    if node.kind() == "number" || node.kind() == "integer" {
        if let Ok(num) = node.utf8_text(text.as_bytes()) {
            segments.push(num.to_string());
        }
        return;
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            // Skip operators and identifiers (column names)
            if child.kind() != "op_other" && child.kind() != "identifier" && child.kind() != "column_reference" {
                collect_segments(&child, text, segments);
            }
        }
    }
}

/// Test JSONB operator detection
#[test]
fn test_jsonb_operator_detection() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
        .expect("Failed to load tree-sitter grammar");

    let test_cases = vec![
        ("SELECT resource->'name' FROM patient", true, "->"),
        ("SELECT resource->>'name' FROM patient", true, "->>"),
        ("SELECT resource#>'{name}' FROM patient", true, "#>"),
        ("SELECT resource#>>'{name}' FROM patient", true, "#>>"),
        ("SELECT resource FROM patient", false, "none"),
        ("SELECT * FROM patient WHERE id = 1", false, "none"),
    ];

    println!("\n===== Testing JSONB Operator Detection =====\n");

    for (sql, should_detect, expected_op) in test_cases {
        println!("SQL: {}", sql);
        println!("Should detect JSONB: {}", should_detect);

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        let found = find_jsonb_expr_in_tree(&root, sql).is_some();

        if found == should_detect {
            println!("✓ Detection correct (found={}, expected={})", found, should_detect);
        } else {
            println!("✗ Detection incorrect (found={}, expected={})", found, should_detect);
        }

        if found {
            println!("  Operator type: {}", expected_op);
        }

        println!();
    }
}

/// Test table name extraction from FROM clause
#[test]
fn test_table_extraction() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
        .expect("Failed to load tree-sitter grammar");

    let test_cases = vec![
        ("SELECT * FROM patient", Some("patient")),
        ("SELECT * FROM public.patient", Some("patient")),
        ("SELECT resource->'name' FROM observation", Some("observation")),
        (
            "SELECT resource->'name' FROM medication_request",
            Some("medication_request"),
        ),
    ];

    println!("\n===== Testing Table Extraction =====\n");

    for (sql, expected_table) in test_cases {
        println!("SQL: {}", sql);
        println!("Expected table: {:?}", expected_table);

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        let found_table = find_table_name(&root, sql);
        println!("Found table: {:?}", found_table);

        if found_table.as_deref() == expected_table {
            println!("✓ Table extraction successful");
        } else {
            println!("✗ Table extraction mismatch");
        }

        println!();
    }
}

/// Helper: Find table name in FROM clause
fn find_table_name(node: &tree_sitter::Node, text: &str) -> Option<String> {
    if node.kind() == "from" {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "relation" {
                    if let Ok(table) = child.utf8_text(text.as_bytes()) {
                        let cleaned = table.split('.').last().unwrap_or(table);
                        return Some(cleaned.to_string());
                    }
                }
            }
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(table) = find_table_name(&child, text) {
                return Some(table);
            }
        }
    }

    None
}

/// Test incomplete JSONB expressions (critical for LSP)
#[test]
fn test_incomplete_jsonb_expressions() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
        .expect("Failed to load tree-sitter grammar");

    let test_cases = vec![
        (
            "SELECT resource-> FROM patient",
            "Operator at end with FROM",
            true,
        ),
        (
            "SELECT resource->'name'-> FROM patient",
            "Chained operator at end",
            true,
        ),
        (
            "SELECT resource->'name'->'given'->",
            "Operator at EOF",
            true,
        ),
    ];

    println!("\n===== Testing Incomplete JSONB Expressions =====\n");

    for (sql, description, should_parse) in test_cases {
        println!("Test: {}", description);
        println!("SQL:  {}", sql);

        let tree = parser.parse(sql, None);

        match tree {
            Some(tree) => {
                let has_errors = tree.root_node().has_error();
                println!("✓ Parsed (has_errors={})", has_errors);

                // Even with errors, we should be able to find JSONB operators
                let found_jsonb = find_jsonb_expr_in_tree(&tree.root_node(), sql).is_some();
                println!("  JSONB detected: {}", found_jsonb);

                if found_jsonb == should_parse {
                    println!("  ✓ JSONB detection correct");
                } else {
                    println!("  ✗ JSONB detection failed");
                }
            }
            None => {
                println!("✗ Parse failed completely");
            }
        }

        println!();
    }
}
