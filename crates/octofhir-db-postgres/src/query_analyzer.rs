//! Query Analysis and Index Suggestion Module
//!
//! This module provides tools for analyzing PostgreSQL query execution plans
//! and suggesting index optimizations for FHIR resource queries.
//!
//! ## Features
//!
//! - Parse EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) output
//! - Identify sequential scans that could benefit from indexes
//! - Generate index creation suggestions
//! - Track and log slow queries with recommendations
//!
//! ## Example
//!
//! ```ignore
//! use octofhir_db_postgres::query_analyzer::{QueryAnalyzer, AnalyzerConfig};
//!
//! let analyzer = QueryAnalyzer::new(AnalyzerConfig::default());
//!
//! // Analyze a query
//! let analysis = analyzer.analyze_query(&pool, sql, &params).await?;
//!
//! // Get index suggestions
//! for suggestion in analysis.index_suggestions {
//!     println!("Suggested index: {}", suggestion.create_statement);
//! }
//! ```

use serde::{Deserialize, Serialize};
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thiserror::Error;
use tracing::{info, warn};

/// Default threshold for slow queries in milliseconds
const DEFAULT_SLOW_QUERY_MS: u64 = 100;

/// Maximum number of slow queries to track
const MAX_SLOW_QUERIES: usize = 100;

/// Errors that can occur during query analysis.
#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("Failed to execute EXPLAIN: {0}")]
    ExplainFailed(String),

    #[error("Failed to parse query plan: {0}")]
    ParseFailed(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx_core::error::Error),
}

/// Configuration for the query analyzer.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Threshold in milliseconds for considering a query "slow"
    pub slow_query_threshold_ms: u64,
    /// Whether to automatically log slow queries
    pub auto_log_slow_queries: bool,
    /// Whether to collect detailed statistics
    pub collect_statistics: bool,
    /// Maximum queries to analyze per second (rate limiting)
    pub max_analysis_rate: Option<u32>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            slow_query_threshold_ms: DEFAULT_SLOW_QUERY_MS,
            auto_log_slow_queries: true,
            collect_statistics: true,
            max_analysis_rate: Some(10),
        }
    }
}

impl AnalyzerConfig {
    /// Create a new analyzer config with custom slow query threshold.
    pub fn with_slow_query_threshold(mut self, ms: u64) -> Self {
        self.slow_query_threshold_ms = ms;
        self
    }

    /// Enable or disable auto-logging of slow queries.
    pub fn with_auto_log(mut self, enabled: bool) -> Self {
        self.auto_log_slow_queries = enabled;
        self
    }
}

/// Result of analyzing a query's execution plan.
#[derive(Debug, Clone)]
pub struct QueryAnalysis {
    /// The original SQL query
    pub sql: String,
    /// Total execution time in milliseconds
    pub execution_time_ms: f64,
    /// Planning time in milliseconds
    pub planning_time_ms: f64,
    /// Whether the query is considered slow
    pub is_slow: bool,
    /// Indexes used by the query
    pub indexes_used: Vec<IndexUsage>,
    /// Sequential scans detected
    pub sequential_scans: Vec<SeqScanInfo>,
    /// Suggested indexes to create
    pub index_suggestions: Vec<IndexSuggestion>,
    /// Total rows scanned
    pub rows_scanned: u64,
    /// Total rows returned
    pub rows_returned: u64,
    /// Buffer statistics
    pub buffer_stats: BufferStats,
}

/// Information about an index used in a query.
#[derive(Debug, Clone)]
pub struct IndexUsage {
    /// Name of the index
    pub index_name: String,
    /// Table the index is on
    pub table_name: String,
    /// Type of scan (Index Scan, Index Only Scan, Bitmap Index Scan)
    pub scan_type: String,
    /// Estimated cost
    pub cost: f64,
    /// Rows returned by this node
    pub rows: u64,
}

/// Information about a sequential scan.
#[derive(Debug, Clone)]
pub struct SeqScanInfo {
    /// Table being scanned
    pub table_name: String,
    /// Filter condition if any
    pub filter: Option<String>,
    /// Estimated rows
    pub estimated_rows: u64,
    /// Actual rows (if ANALYZE was used)
    pub actual_rows: Option<u64>,
    /// Whether this scan is problematic (large table, no filter)
    pub is_problematic: bool,
}

/// A suggested index to create.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSuggestion {
    /// Table to create the index on
    pub table_name: String,
    /// Columns to index
    pub columns: Vec<String>,
    /// Suggested index name
    pub index_name: String,
    /// SQL CREATE INDEX statement
    pub create_statement: String,
    /// Reason for suggesting this index
    pub reason: String,
    /// Estimated impact (High, Medium, Low)
    pub impact: SuggestionImpact,
}

/// Impact level for an index suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionImpact {
    High,
    Medium,
    Low,
}

/// Buffer usage statistics from query execution.
#[derive(Debug, Clone, Default)]
pub struct BufferStats {
    /// Shared buffer hits
    pub shared_hit: u64,
    /// Shared buffer reads
    pub shared_read: u64,
    /// Local buffer hits
    pub local_hit: u64,
    /// Local buffer reads
    pub local_read: u64,
    /// Temp buffer reads
    pub temp_read: u64,
    /// Temp buffer writes
    pub temp_write: u64,
}

/// PostgreSQL query plan node from EXPLAIN JSON output.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)] // Fields are used in JSON deserialization
struct QueryPlanNode {
    node_type: String,
    #[serde(default)]
    relation_name: Option<String>,
    #[serde(default)]
    index_name: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    index_cond: Option<String>,
    #[serde(default)]
    startup_cost: f64,
    #[serde(default)]
    total_cost: f64,
    #[serde(default)]
    plan_rows: u64,
    #[serde(default)]
    actual_rows: Option<u64>,
    #[serde(default)]
    actual_total_time: Option<f64>,
    #[serde(default)]
    shared_hit_blocks: Option<u64>,
    #[serde(default)]
    shared_read_blocks: Option<u64>,
    #[serde(default)]
    local_hit_blocks: Option<u64>,
    #[serde(default)]
    local_read_blocks: Option<u64>,
    #[serde(default)]
    temp_read_blocks: Option<u64>,
    #[serde(default)]
    temp_written_blocks: Option<u64>,
    #[serde(default)]
    plans: Vec<QueryPlanNode>,
}

/// Top-level EXPLAIN JSON output.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExplainOutput {
    plan: QueryPlanNode,
    #[serde(default)]
    planning_time: f64,
    #[serde(default)]
    execution_time: f64,
}

/// Statistics tracked by the analyzer.
#[derive(Debug, Default)]
pub struct AnalyzerStats {
    /// Total queries analyzed
    pub queries_analyzed: AtomicU64,
    /// Slow queries detected
    pub slow_queries: AtomicU64,
    /// Sequential scans detected
    pub seq_scans_detected: AtomicU64,
    /// Index suggestions generated
    pub suggestions_generated: AtomicU64,
}

impl AnalyzerStats {
    /// Get a snapshot of current statistics.
    pub fn snapshot(&self) -> AnalyzerStatsSnapshot {
        AnalyzerStatsSnapshot {
            queries_analyzed: self.queries_analyzed.load(Ordering::Relaxed),
            slow_queries: self.slow_queries.load(Ordering::Relaxed),
            seq_scans_detected: self.seq_scans_detected.load(Ordering::Relaxed),
            suggestions_generated: self.suggestions_generated.load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time snapshot of analyzer statistics.
#[derive(Debug, Clone)]
pub struct AnalyzerStatsSnapshot {
    pub queries_analyzed: u64,
    pub slow_queries: u64,
    pub seq_scans_detected: u64,
    pub suggestions_generated: u64,
}

/// A record of a slow query.
#[derive(Debug, Clone)]
pub struct SlowQueryRecord {
    /// The SQL query
    pub sql: String,
    /// Execution time in milliseconds
    pub execution_time_ms: f64,
    /// When the query was executed
    pub timestamp: Instant,
    /// Index suggestions for this query
    pub suggestions: Vec<IndexSuggestion>,
}

/// Query analyzer for PostgreSQL FHIR queries.
pub struct QueryAnalyzer {
    config: AnalyzerConfig,
    stats: Arc<AnalyzerStats>,
    slow_queries: Arc<dashmap::DashMap<u64, SlowQueryRecord>>,
    slow_query_counter: AtomicU64,
}

impl QueryAnalyzer {
    /// Create a new query analyzer with the given configuration.
    pub fn new(config: AnalyzerConfig) -> Self {
        Self {
            config,
            stats: Arc::new(AnalyzerStats::default()),
            slow_queries: Arc::new(dashmap::DashMap::new()),
            slow_query_counter: AtomicU64::new(0),
        }
    }

    /// Create a query analyzer with default configuration.
    pub fn default_analyzer() -> Self {
        Self::new(AnalyzerConfig::default())
    }

    /// Analyze a query and return detailed execution information.
    ///
    /// This runs EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) on the query.
    pub async fn analyze_query(
        &self,
        pool: &PgPool,
        sql: &str,
    ) -> Result<QueryAnalysis, AnalyzerError> {
        // Build EXPLAIN query
        let explain_sql = format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {}", sql);

        // Execute EXPLAIN
        let row: (serde_json::Value,) = query_as(&explain_sql)
            .fetch_one(pool)
            .await
            .map_err(|e| AnalyzerError::ExplainFailed(e.to_string()))?;

        // Parse the JSON output
        let explain_output = self.parse_explain_output(&row.0)?;

        // Build analysis
        let analysis = self.build_analysis(sql, &explain_output);

        // Update statistics
        self.stats.queries_analyzed.fetch_add(1, Ordering::Relaxed);

        if analysis.is_slow {
            self.stats.slow_queries.fetch_add(1, Ordering::Relaxed);
            self.record_slow_query(&analysis);
        }

        if !analysis.sequential_scans.is_empty() {
            self.stats
                .seq_scans_detected
                .fetch_add(analysis.sequential_scans.len() as u64, Ordering::Relaxed);
        }

        if !analysis.index_suggestions.is_empty() {
            self.stats
                .suggestions_generated
                .fetch_add(analysis.index_suggestions.len() as u64, Ordering::Relaxed);
        }

        // Auto-log if enabled
        if self.config.auto_log_slow_queries && analysis.is_slow {
            self.log_slow_query(&analysis);
        }

        Ok(analysis)
    }

    /// Analyze a query without executing it (planning only).
    pub async fn analyze_plan_only(
        &self,
        pool: &PgPool,
        sql: &str,
    ) -> Result<QueryAnalysis, AnalyzerError> {
        let explain_sql = format!("EXPLAIN (FORMAT JSON) {}", sql);

        let row: (serde_json::Value,) = query_as(&explain_sql)
            .fetch_one(pool)
            .await
            .map_err(|e| AnalyzerError::ExplainFailed(e.to_string()))?;

        let explain_output = self.parse_explain_output(&row.0)?;
        let mut analysis = self.build_analysis(sql, &explain_output);

        // Without ANALYZE, we don't have actual execution time
        analysis.is_slow = false;

        Ok(analysis)
    }

    /// Get index suggestions for a table based on common FHIR query patterns.
    pub fn suggest_fhir_indexes(&self, table_name: &str) -> Vec<IndexSuggestion> {
        let table_lower = table_name.to_lowercase();
        let mut suggestions = Vec::new();

        // Common FHIR resource index suggestions
        // Index on resource type for polymorphic queries
        suggestions.push(IndexSuggestion {
            table_name: table_lower.clone(),
            columns: vec!["resource->>'resourceType'".to_string()],
            index_name: format!("idx_{}_resource_type", table_lower),
            create_statement: format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_resource_type ON \"{}\" ((resource->>'resourceType'))",
                table_lower, table_lower
            ),
            reason: "Index on resourceType for efficient resource type filtering".to_string(),
            impact: SuggestionImpact::High,
        });

        // GIN index on resource JSONB for general queries
        suggestions.push(IndexSuggestion {
            table_name: table_lower.clone(),
            columns: vec!["resource".to_string()],
            index_name: format!("idx_{}_resource_gin", table_lower),
            create_statement: format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_resource_gin ON \"{}\" USING GIN (resource jsonb_path_ops)",
                table_lower, table_lower
            ),
            reason: "GIN index for efficient JSONB containment queries".to_string(),
            impact: SuggestionImpact::Medium,
        });

        // Index on lastUpdated for history queries
        suggestions.push(IndexSuggestion {
            table_name: table_lower.clone(),
            columns: vec!["ts".to_string()],
            index_name: format!("idx_{}_ts", table_lower),
            create_statement: format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_ts ON \"{}\" (ts DESC)",
                table_lower, table_lower
            ),
            reason: "Index on timestamp for _lastUpdated searches and history".to_string(),
            impact: SuggestionImpact::High,
        });

        suggestions
    }

    /// Check if a query would benefit from analysis (not too simple).
    pub fn should_analyze(&self, sql: &str) -> bool {
        // Skip very simple queries
        let sql_upper = sql.to_uppercase();
        if sql_upper.starts_with("SELECT 1")
            || sql_upper.starts_with("SELECT COUNT(*) FROM")
            || sql_upper.contains("LIMIT 1")
        {
            return false;
        }

        // Analyze queries with WHERE clauses or JOINs
        sql_upper.contains("WHERE") || sql_upper.contains("JOIN")
    }

    /// Get analyzer statistics.
    pub fn stats(&self) -> AnalyzerStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get recent slow queries.
    pub fn recent_slow_queries(&self) -> Vec<SlowQueryRecord> {
        self.slow_queries
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Clear slow query history.
    pub fn clear_slow_queries(&self) {
        self.slow_queries.clear();
    }

    // Private methods

    fn parse_explain_output(
        &self,
        json: &serde_json::Value,
    ) -> Result<ExplainOutput, AnalyzerError> {
        // PostgreSQL returns an array with one element
        let plan_array = json
            .as_array()
            .ok_or_else(|| AnalyzerError::ParseFailed("Expected array".to_string()))?;

        let plan_obj = plan_array
            .first()
            .ok_or_else(|| AnalyzerError::ParseFailed("Empty plan array".to_string()))?;

        serde_json::from_value(plan_obj.clone())
            .map_err(|e| AnalyzerError::ParseFailed(e.to_string()))
    }

    fn build_analysis(&self, sql: &str, explain: &ExplainOutput) -> QueryAnalysis {
        let mut indexes_used = Vec::new();
        let mut sequential_scans = Vec::new();
        let mut buffer_stats = BufferStats::default();
        let mut rows_scanned = 0u64;
        let mut rows_returned = 0u64;

        // Traverse the plan tree
        Self::traverse_plan(
            &explain.plan,
            &mut indexes_used,
            &mut sequential_scans,
            &mut buffer_stats,
            &mut rows_scanned,
            &mut rows_returned,
        );

        // Generate index suggestions
        let index_suggestions = self.generate_suggestions(&sequential_scans);

        // Determine if slow
        let is_slow = explain.execution_time >= self.config.slow_query_threshold_ms as f64;

        QueryAnalysis {
            sql: sql.to_string(),
            execution_time_ms: explain.execution_time,
            planning_time_ms: explain.planning_time,
            is_slow,
            indexes_used,
            sequential_scans,
            index_suggestions,
            rows_scanned,
            rows_returned,
            buffer_stats,
        }
    }

    fn traverse_plan(
        node: &QueryPlanNode,
        indexes_used: &mut Vec<IndexUsage>,
        sequential_scans: &mut Vec<SeqScanInfo>,
        buffer_stats: &mut BufferStats,
        rows_scanned: &mut u64,
        rows_returned: &mut u64,
    ) {
        // Accumulate buffer stats
        buffer_stats.shared_hit += node.shared_hit_blocks.unwrap_or(0);
        buffer_stats.shared_read += node.shared_read_blocks.unwrap_or(0);
        buffer_stats.local_hit += node.local_hit_blocks.unwrap_or(0);
        buffer_stats.local_read += node.local_read_blocks.unwrap_or(0);
        buffer_stats.temp_read += node.temp_read_blocks.unwrap_or(0);
        buffer_stats.temp_write += node.temp_written_blocks.unwrap_or(0);

        // Track rows
        *rows_scanned += node.plan_rows;
        if let Some(actual) = node.actual_rows {
            *rows_returned += actual;
        }

        // Check node type
        match node.node_type.as_str() {
            "Index Scan" | "Index Only Scan" | "Bitmap Index Scan" => {
                if let Some(index_name) = &node.index_name {
                    indexes_used.push(IndexUsage {
                        index_name: index_name.clone(),
                        table_name: node.relation_name.clone().unwrap_or_default(),
                        scan_type: node.node_type.clone(),
                        cost: node.total_cost,
                        rows: node.actual_rows.unwrap_or(node.plan_rows),
                    });
                }
            }
            "Seq Scan" => {
                let table_name = node.relation_name.clone().unwrap_or_default();
                let is_problematic = node.plan_rows > 1000 || node.filter.is_some();

                sequential_scans.push(SeqScanInfo {
                    table_name,
                    filter: node.filter.clone(),
                    estimated_rows: node.plan_rows,
                    actual_rows: node.actual_rows,
                    is_problematic,
                });
            }
            _ => {}
        }

        // Recurse into child nodes
        for child in &node.plans {
            Self::traverse_plan(
                child,
                indexes_used,
                sequential_scans,
                buffer_stats,
                rows_scanned,
                rows_returned,
            );
        }
    }

    fn generate_suggestions(&self, seq_scans: &[SeqScanInfo]) -> Vec<IndexSuggestion> {
        let mut suggestions = Vec::new();

        for scan in seq_scans {
            if !scan.is_problematic {
                continue;
            }

            // Parse filter to extract columns
            if let Some(filter) = &scan.filter {
                let columns = self.extract_filter_columns(filter);
                if !columns.is_empty() {
                    let index_name =
                        format!("idx_{}_{}_suggested", scan.table_name, columns.join("_"));

                    let column_list = columns.join(", ");
                    suggestions.push(IndexSuggestion {
                        table_name: scan.table_name.clone(),
                        columns: columns.clone(),
                        index_name: index_name.clone(),
                        create_statement: format!(
                            "CREATE INDEX IF NOT EXISTS {} ON \"{}\" ({})",
                            index_name, scan.table_name, column_list
                        ),
                        reason: format!(
                            "Sequential scan on {} with filter: {}",
                            scan.table_name, filter
                        ),
                        impact: if scan.estimated_rows > 10000 {
                            SuggestionImpact::High
                        } else if scan.estimated_rows > 1000 {
                            SuggestionImpact::Medium
                        } else {
                            SuggestionImpact::Low
                        },
                    });
                }
            }
        }

        suggestions
    }

    fn extract_filter_columns(&self, filter: &str) -> Vec<String> {
        let mut columns = Vec::new();

        // Simple pattern matching for common filter patterns
        // Look for patterns like: column_name = value, column_name IS NOT NULL, etc.
        let patterns = [
            r"\((\w+)\s*=",
            r"\((\w+)\s*IS",
            r"(\w+)\s*~~",
            r"(\w+)\s*ILIKE",
            r"(\w+)\s*LIKE",
        ];

        for pattern in patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                for cap in re.captures_iter(filter) {
                    if let Some(col) = cap.get(1) {
                        let col_name = col.as_str().to_string();
                        if !columns.contains(&col_name)
                            && !col_name.eq_ignore_ascii_case("NULL")
                            && !col_name.eq_ignore_ascii_case("TRUE")
                            && !col_name.eq_ignore_ascii_case("FALSE")
                        {
                            columns.push(col_name);
                        }
                    }
                }
            }
        }

        columns
    }

    fn record_slow_query(&self, analysis: &QueryAnalysis) {
        let id = self.slow_query_counter.fetch_add(1, Ordering::Relaxed);

        // Evict old entries if at capacity
        if self.slow_queries.len() >= MAX_SLOW_QUERIES {
            // Remove oldest entry
            if let Some(oldest_key) = self.slow_queries.iter().next().map(|e| *e.key()) {
                self.slow_queries.remove(&oldest_key);
            }
        }

        self.slow_queries.insert(
            id,
            SlowQueryRecord {
                sql: analysis.sql.clone(),
                execution_time_ms: analysis.execution_time_ms,
                timestamp: Instant::now(),
                suggestions: analysis.index_suggestions.clone(),
            },
        );
    }

    fn log_slow_query(&self, analysis: &QueryAnalysis) {
        warn!(
            sql = %analysis.sql,
            execution_time_ms = analysis.execution_time_ms,
            rows_scanned = analysis.rows_scanned,
            seq_scans = analysis.sequential_scans.len(),
            suggestions = analysis.index_suggestions.len(),
            "Slow query detected"
        );

        for suggestion in &analysis.index_suggestions {
            info!(
                table = %suggestion.table_name,
                impact = ?suggestion.impact,
                reason = %suggestion.reason,
                create_statement = %suggestion.create_statement,
                "Index suggestion"
            );
        }
    }
}

impl std::fmt::Debug for QueryAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryAnalyzer")
            .field("config", &self.config)
            .field("stats", &self.stats.snapshot())
            .field("slow_queries_count", &self.slow_queries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_config_default() {
        let config = AnalyzerConfig::default();
        assert_eq!(config.slow_query_threshold_ms, DEFAULT_SLOW_QUERY_MS);
        assert!(config.auto_log_slow_queries);
        assert!(config.collect_statistics);
    }

    #[test]
    fn test_analyzer_config_builder() {
        let config = AnalyzerConfig::default()
            .with_slow_query_threshold(200)
            .with_auto_log(false);

        assert_eq!(config.slow_query_threshold_ms, 200);
        assert!(!config.auto_log_slow_queries);
    }

    #[test]
    fn test_should_analyze() {
        let analyzer = QueryAnalyzer::default_analyzer();

        assert!(analyzer.should_analyze("SELECT * FROM patient WHERE id = $1"));
        assert!(
            analyzer.should_analyze(
                "SELECT * FROM observation o JOIN patient p ON o.subject_id = p.id"
            )
        );
        assert!(!analyzer.should_analyze("SELECT 1"));
        assert!(!analyzer.should_analyze("SELECT COUNT(*) FROM patient"));
    }

    #[test]
    fn test_fhir_index_suggestions() {
        let analyzer = QueryAnalyzer::default_analyzer();
        let suggestions = analyzer.suggest_fhir_indexes("Patient");

        assert!(!suggestions.is_empty());
        assert!(
            suggestions
                .iter()
                .any(|s| s.index_name.contains("resource_type"))
        );
        assert!(suggestions.iter().any(|s| s.index_name.contains("gin")));
        assert!(suggestions.iter().any(|s| s.index_name.contains("ts")));
    }

    #[test]
    fn test_extract_filter_columns() {
        let analyzer = QueryAnalyzer::default_analyzer();

        let columns = analyzer.extract_filter_columns("(status = 'active')");
        assert!(columns.contains(&"status".to_string()));

        let columns = analyzer.extract_filter_columns("(name ILIKE '%smith%')");
        assert!(columns.contains(&"name".to_string()));

        let columns = analyzer.extract_filter_columns("(id IS NOT NULL)");
        assert!(columns.contains(&"id".to_string()));
    }

    #[test]
    fn test_suggestion_impact() {
        assert_eq!(
            serde_json::to_string(&SuggestionImpact::High).unwrap(),
            "\"High\""
        );
    }

    #[test]
    fn test_stats_snapshot() {
        let analyzer = QueryAnalyzer::default_analyzer();
        let stats = analyzer.stats();

        assert_eq!(stats.queries_analyzed, 0);
        assert_eq!(stats.slow_queries, 0);
    }

    #[test]
    fn test_index_suggestion_serialization() {
        let suggestion = IndexSuggestion {
            table_name: "patient".to_string(),
            columns: vec!["name".to_string()],
            index_name: "idx_patient_name".to_string(),
            create_statement: "CREATE INDEX idx_patient_name ON patient (name)".to_string(),
            reason: "Sequential scan detected".to_string(),
            impact: SuggestionImpact::High,
        };

        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("patient"));
        assert!(json.contains("High"));
    }
}
