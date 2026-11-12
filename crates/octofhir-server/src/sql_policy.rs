//! Admission for console SQL. PostgreSQL permissions and READ ONLY transactions
//! remain the execution boundary; a keyword blacklist is not a SQL parser.

use crate::config::SqlMode;
use banshee_parser::SyntaxKind as K;

pub(crate) fn validate(mode: SqlMode, sql: &str) -> Result<(), String> {
    if sql.trim().is_empty() || sql.len() > 64 * 1024 {
        return Err("SQL must contain one statement of at most 64 KiB".into());
    }
    // Bound parser recursion before parsing, including intentionally conservative
    // rejection of unusually deep parentheses inside literals.
    let mut depth = 0usize;
    for ch in sql.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth > 128 {
                    return Err("SQL nesting is too deep".into());
                }
            }
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let parsed = banshee_parser::parse(sql);
    if !parsed.errors().is_empty() {
        return Err("SQL could not be parsed safely".into());
    }
    let root = parsed.syntax();
    if root.children().count() != 1 {
        return Err("Exactly one SQL statement is required".into());
    }
    if mode == SqlMode::Admin {
        return Ok(());
    }
    // Walk nested statements, too: a WITH or EXPLAIN may contain a mutation.
    for node in root.descendants() {
        let kind = node.kind();
        if format!("{kind:?}").ends_with("_STMT") {
            let allowed = matches!(kind, K::SELECT_STMT | K::EXPLAIN_STMT)
                || (mode == SqlMode::Readwrite
                    && matches!(kind, K::INSERT_STMT | K::UPDATE_STMT | K::DELETE_STMT));
            if !allowed {
                return Err(format!("Statement is not allowed in {mode} mode"));
            }
        }
        if kind == K::SELECT_STMT
            && node.descendants_with_tokens().any(|element| {
                element
                    .as_token()
                    .is_some_and(|token| token.kind() == K::INTO_KW)
            })
        {
            return Err("SELECT INTO is not allowed in restricted SQL modes".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_statement_and_cte_bypasses() {
        for sql in [
            "SELECT 1 INTO audit_probe",
            "SELECT 1; SELECT 2",
            "COPY (SELECT 1) TO PROGRAM 'true'",
            "WITH x AS (DELETE FROM patient RETURNING *) SELECT * FROM x",
            "EXPLAIN ANALYZE SELECT 1 INTO audit_probe",
            "SET ROLE postgres",
            "DO $$ BEGIN NULL; END $$",
        ] {
            assert!(validate(SqlMode::Readonly, sql).is_err(), "{sql}");
        }
        assert!(validate(SqlMode::Readwrite, "COPY (SELECT 1) TO PROGRAM 'true'").is_err());
        assert!(
            validate(
                SqlMode::Readwrite,
                "WITH x AS (SELECT 1) DROP TABLE patient"
            )
            .is_err()
        );
    }

    #[test]
    fn allows_literals_comments_and_valid_queries() {
        for sql in [
            "SELECT 'DROP UPDATE INTO' AS text",
            "/* ordinary comment */ SELECT 1;",
            "WITH x AS (SELECT 1 AS n) SELECT n FROM x",
            "EXPLAIN ANALYZE SELECT * FROM patient",
        ] {
            assert!(validate(SqlMode::Readonly, sql).is_ok(), "{sql}");
        }
        assert!(validate(SqlMode::Readwrite, "UPDATE patient SET status = 'active'").is_ok());
    }
}
