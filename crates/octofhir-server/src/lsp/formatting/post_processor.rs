//! Advanced SQL formatting post-processor
//!
//! This module provides sophisticated formatting transformations on top of pg_query's
//! deparse output, including smart indentation, keyword alignment, and line wrapping.
//!
//! All formatting rules are hardcoded based on sqlstyle.guide.

use super::formatter::FormatResult;
use super::style;
use std::collections::HashSet;

/// Post-processor for applying sqlstyle.guide formatting to deparsed SQL.
/// All rules are hardcoded with no configuration options.
pub struct FormattingPostProcessor;

impl FormattingPostProcessor {
    /// Create a new post-processor with hardcoded sqlstyle.guide rules.
    pub fn new() -> Self {
        Self
    }

    /// Apply all sqlstyle.guide formatting transformations to SQL.
    pub fn process(&self, sql: &str) -> FormatResult<String> {
        let mut output = sql.to_string();

        // Apply transformations in order:
        // 1. Split into multiple lines (pg_query deparse outputs single-line SQL)
        output = self.split_into_lines(&output)?;

        // 2. Right alignment (always enabled per sqlstyle.guide)
        output = self.apply_right_alignment(&output)?;

        // 3. PostgreSQL-specific formatting (JSONB, arrays, etc.)
        let pg_formatter = super::postgres_extensions::PostgresExtensionFormatter::new();
        output = pg_formatter.format(&output)?;

        Ok(output)
    }

    /// Split single-line SQL into multiple lines based on major keywords.
    ///
    /// pg_query's deparse() can output single-line or multi-line SQL.
    /// We split on major clause keywords and also split AND/OR within ON clauses.
    fn split_into_lines(&self, sql: &str) -> FormatResult<String> {
        // Keywords that should start a new line (major clauses only)
        let multi_word_keywords = vec![
            " LEFT JOIN ", " RIGHT JOIN ", " INNER JOIN ", " OUTER JOIN ",
            " CROSS JOIN ", " FULL JOIN ",
            " FULL OUTER JOIN ", " LEFT OUTER JOIN ", " RIGHT OUTER JOIN ",
            " GROUP BY ", " HAVING ", " ORDER BY ",
            " UNION ALL ",
        ];

        let single_word_keywords = vec![
            " FROM ", " WHERE ", " JOIN ",
            " LIMIT ", " OFFSET ",
            " UNION ", " INTERSECT ", " EXCEPT ",
            " ON ", " WITH ",
        ];

        let mut result = sql.to_string();

        // Step 1: Replace multi-word keywords with placeholders to protect them
        let mut replacements = Vec::new();
        for (idx, keyword) in multi_word_keywords.iter().enumerate() {
            let keyword_upper = keyword.to_uppercase();
            let placeholder = format!("%%PLACEHOLDER{}%%", idx);
            result = result.replace(&keyword_upper, &placeholder);
            replacements.push((placeholder, keyword_upper));
        }

        // Step 2: Replace single-word keywords (now won't match inside placeholders)
        for keyword in single_word_keywords {
            let keyword_upper = keyword.to_uppercase();
            result = result.replace(&keyword_upper, &format!("\n{} ", keyword_upper.trim()));
        }

        // Step 3: Replace placeholders with actual multi-word keywords
        for (placeholder, keyword_upper) in replacements {
            result = result.replace(&placeholder, &format!("\n{} ", keyword_upper.trim()));
        }

        // Step 4: Split AND/OR within ON clauses onto separate lines
        // Process line by line to detect ON clause context
        let lines: Vec<&str> = result.lines().collect();
        let mut final_result = Vec::new();
        let mut in_on_clause = false;

        for line in lines {
            let trimmed = line.trim().to_uppercase();

            if trimmed.starts_with("ON ") {
                // Start of ON clause
                in_on_clause = true;
                // Split AND/OR in this ON line
                let processed = self.split_and_or_in_line(line);
                final_result.push(processed);
            } else if trimmed.starts_with("WHERE ")
                || trimmed.starts_with("SELECT ")
                || trimmed.starts_with("FROM ")
                || trimmed.starts_with("GROUP BY")
                || trimmed.starts_with("HAVING")
                || trimmed.starts_with("ORDER BY")
                || trimmed.starts_with("LIMIT")
                || trimmed.starts_with("JOIN")
                || trimmed.starts_with("LEFT JOIN")
                || trimmed.starts_with("RIGHT JOIN")
                || trimmed.starts_with("INNER JOIN")
            {
                // End of ON clause - major keyword found
                in_on_clause = false;
                final_result.push(line.to_string());
            } else if in_on_clause {
                // Still in ON clause - split AND/OR
                let processed = self.split_and_or_in_line(line);
                final_result.push(processed);
            } else {
                // Not in ON clause - keep as-is
                final_result.push(line.to_string());
            }
        }

        Ok(final_result.join("\n"))
    }

    /// Split AND/OR in a single line
    fn split_and_or_in_line(&self, line: &str) -> String {
        line.replace(" AND ", "\nAND ")
            .replace(" OR ", "\nOR ")
    }


    /// Apply right-alignment to major SQL keywords (sqlstyle.guide).
    /// Creates the "river" effect with keywords aligned on their right edge.
    /// JOINs get special indentation (5 spaces MORE than FROM, not part of river).
    fn apply_right_alignment(&self, sql: &str) -> FormatResult<String> {
        let major_keywords = style::get_major_keywords();
        let logical_operators = vec!["AND", "OR"];
        let join_keywords = vec![
            "JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN", "OUTER JOIN",
            "CROSS JOIN", "FULL JOIN", "FULL OUTER JOIN",
            "LEFT OUTER JOIN", "RIGHT OUTER JOIN",
        ];
        let lines: Vec<&str> = sql.lines().collect();

        // Find maximum keyword length for alignment
        let max_keyword_len = major_keywords.iter()
            .map(|k| k.len())
            .max()
            .unwrap_or(style::KEYWORD_PADDING_WIDTH);

        // First pass: determine FROM indentation
        let from_indent = lines.iter()
            .find(|line| line.trim().to_uppercase().starts_with("FROM "))
            .map(|line| {
                let trimmed = line.trim();
                let padding = max_keyword_len - "FROM".len();
                padding
            })
            .unwrap_or(0);

        // JOIN should be indented 5 spaces MORE than FROM to align with table content
        let join_indent = from_indent + 5;

        let mut result = Vec::new();
        let mut in_on_clause = false;

        for line in lines {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                result.push(String::new());
                continue;
            }

            // Check if line starts with a JOIN keyword (must check before major keywords)
            if let Some(_join_kw) = self.find_starting_join(trimmed, &join_keywords) {
                // JOINs are indented 5 spaces more than FROM per sqlstyle.guide
                let alignment = " ".repeat(join_indent);
                result.push(format!("{}{}", alignment, trimmed));
                in_on_clause = false;
            }
            // Check if line starts with ON (same indentation as JOIN)
            else if trimmed.to_uppercase().starts_with("ON ") {
                // ON gets same indentation as JOIN
                let alignment = " ".repeat(join_indent);
                result.push(format!("{}{}", alignment, trimmed));
                in_on_clause = true;
            }
            // Check if line starts with a logical operator (AND/OR)
            else if logical_operators.contains(&trimmed.split_whitespace().next().unwrap_or("").to_uppercase().as_str()) {
                if in_on_clause {
                    // AND/OR within ON clause: 3 more spaces than ON (10 total if ON is at 7)
                    let alignment = " ".repeat(join_indent + 3);
                    result.push(format!("{}{}", alignment, trimmed));
                } else {
                    // AND/OR in WHERE clause: extra indentation per sqlstyle.guide
                    let operator_padding = max_keyword_len - trimmed.split_whitespace().next().unwrap_or("").len() + 3;
                    let alignment = " ".repeat(operator_padding);
                    result.push(format!("{}{}", alignment, trimmed));
                    in_on_clause = false;
                }
            }
            // Check if line starts with a major keyword
            else if let Some(keyword) = self.find_starting_keyword(trimmed, &major_keywords) {
                // Calculate leading spaces needed for right-alignment
                // Right alignment: keywords align on their RIGHT edge to create "river"
                let padding = max_keyword_len - keyword.len();
                let alignment = " ".repeat(padding);
                result.push(format!("{}{}", alignment, trimmed));
                in_on_clause = false;
            } else {
                // Not a keyword line - preserve as-is (could be continuation, values, etc.)
                result.push(trimmed.to_string());
            }
        }

        let aligned = result.join("\n");

        // Remove common leading whitespace so document starts at column 0
        Ok(self.remove_common_indent(&aligned))
    }

    /// Remove common leading whitespace from all non-empty lines.
    fn remove_common_indent(&self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();

        // Find minimum indentation (excluding empty lines)
        let min_indent = lines.iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);

        if min_indent == 0 {
            return text.to_string();
        }

        // Remove min_indent spaces from each line
        lines.iter()
            .map(|line| {
                if line.len() >= min_indent && !line.trim().is_empty() {
                    &line[min_indent..]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }


    /// Find which major keyword a line starts with.
    ///
    /// Checks two-word keywords first (e.g., "LEFT JOIN") before single-word keywords (e.g., "LEFT")
    /// to ensure longest match wins.
    fn find_starting_keyword(&self, line: &str, keywords: &HashSet<&str>) -> Option<String> {
        let line_upper = line.to_uppercase();
        let words: Vec<&str> = line_upper.split_whitespace().collect();

        if words.is_empty() {
            return None;
        }

        // Check two-word keywords FIRST (e.g., "LEFT JOIN", "ORDER BY")
        // This ensures "LEFT JOIN" matches before "LEFT"
        if words.len() >= 2 {
            let two_word = format!("{} {}", words[0], words[1]);
            if keywords.contains(two_word.as_str()) {
                return Some(two_word);
            }
        }

        // Check single-word keywords
        if keywords.contains(words[0]) {
            return Some(words[0].to_string());
        }

        None
    }

    /// Find which JOIN keyword a line starts with.
    ///
    /// Checks multi-word JOINs first (e.g., "LEFT JOIN") before single-word JOIN.
    fn find_starting_join(&self, line: &str, join_keywords: &[&str]) -> Option<String> {
        let line_upper = line.to_uppercase();

        // Sort by length descending to check longest matches first
        // This ensures "LEFT OUTER JOIN" matches before "LEFT JOIN" before "JOIN"
        let mut sorted_joins: Vec<&str> = join_keywords.to_vec();
        sorted_joins.sort_by_key(|k| std::cmp::Reverse(k.len()));

        for join_kw in sorted_joins {
            if line_upper.starts_with(join_kw) {
                // Make sure it's followed by whitespace or end of string
                let after_kw = &line_upper[join_kw.len()..];
                if after_kw.is_empty() || after_kw.starts_with(' ') {
                    return Some(join_kw.to_string());
                }
            }
        }

        None
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_right_alignment() {
        let processor = FormattingPostProcessor::new();

        let sql = "SELECT id, name\nFROM patient\nWHERE active = true";
        let result = processor.apply_right_alignment(sql).unwrap();

        // All keywords should be right-aligned per sqlstyle.guide
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines[0].contains("SELECT"));
        assert!(lines[1].contains("FROM"));  // FROM gets padding for alignment
        assert!(lines[2].contains("WHERE"));  // WHERE gets padding
    }

    #[test]
    fn test_process_full() {
        let processor = FormattingPostProcessor::new();

        let sql = "SELECT id, name FROM patient WHERE active = true";
        let result = processor.process(sql).unwrap();

        // Should apply all transformations
        assert!(!result.is_empty());
        assert!(result.contains("SELECT"));
    }

    #[test]
    fn test_find_starting_keyword() {
        let processor = FormattingPostProcessor::new();
        let keywords = style::get_major_keywords();

        assert_eq!(
            processor.find_starting_keyword("SELECT id FROM patient", &keywords),
            Some("SELECT".to_string())
        );

        assert_eq!(
            processor.find_starting_keyword("LEFT JOIN observation ON patient.id = observation.subject", &keywords),
            Some("LEFT JOIN".to_string())
        );

        assert_eq!(
            processor.find_starting_keyword("ORDER BY id", &keywords),
            Some("ORDER BY".to_string())
        );
    }

    #[test]
    fn test_split_into_lines() {
        let processor = FormattingPostProcessor::new();
        let sql = "SELECT id FROM patient WHERE active=true";
        let result = processor.split_into_lines(sql).unwrap();

        // Should split on major keywords
        assert!(result.contains('\n'));
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 3); // At least SELECT, FROM, WHERE lines
    }
}
