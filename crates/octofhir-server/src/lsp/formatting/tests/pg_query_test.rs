//! Tests for pg_query deparse functionality

#[test]
fn test_pg_query_deparse() {
    let queries = vec![
        "select id,name,email from patient where active=true",
        "SELECT * FROM patient LEFT JOIN observation ON patient.id=observation.subject",
        "INSERT INTO patient (id, name) VALUES (1, 'John')",
        "CREATE TABLE test (id INT, name TEXT)",
        "WITH cte AS (SELECT * FROM patient) SELECT * FROM cte",
    ];

    for sql in queries {
        println!("\n=== Original ===");
        println!("{}", sql);

        match pg_query::parse(sql) {
            Ok(result) => {
                // Show AST structure
                println!("=== AST Nodes ===");
                for (i, (node_ref, _offset, _context, _location)) in result.protobuf.nodes().iter().enumerate() {
                    println!("Node {}: {:?}", i, node_ref);
                }

                // Show deparsed SQL
                match result.deparse() {
                    Ok(deparsed) => {
                        println!("=== Deparsed ===");
                        println!("{}", deparsed);
                    }
                    Err(e) => println!("Deparse error: {}", e),
                }
            }
            Err(e) => println!("Parse error: {}", e),
        }
    }
}

#[test]
fn test_pg_query_ast_walking() {
    use pg_query::NodeRef;

    let sql = "SELECT id, name FROM patient WHERE active = true";
    println!("\n=== Testing AST Walking ===");
    println!("SQL: {}", sql);

    let result = pg_query::parse(sql).unwrap();

    println!("\n=== Walking AST ===");
    for (i, (node_ref, _offset, _context, _location)) in result.protobuf.nodes().iter().enumerate() {
        match node_ref {
            NodeRef::SelectStmt(select) => {
                println!("Found SelectStmt at node {}", i);
                println!("  target_list: {:?}", select.target_list.len());
                println!("  from_clause: {:?}", select.from_clause.len());
                println!("  where_clause: {:?}", select.where_clause.is_some());
            }
            _ => println!("Node {}: Other type", i),
        }
    }

    // Test deparse
    let deparsed = result.deparse().unwrap();
    println!("\n=== Deparsed ===");
    println!("{}", deparsed);

    // Test that deparsed SQL can be parsed again (basic round-trip validation)
    let re_parsed = pg_query::parse(&deparsed);
    assert!(re_parsed.is_ok(), "Deparsed SQL should be valid and parseable");
    println!("\n✓ Round-trip validation passed - deparsed SQL is valid!");
}
