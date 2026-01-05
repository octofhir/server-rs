//! Configuration types for the PostgreSQL storage backend.

use serde::{Deserialize, Serialize};

/// Configuration for the PostgreSQL storage backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// Connection URL: `postgres://user:pass@host:port/database`
    pub url: String,

    /// Connection pool size (maximum number of connections).
    pub pool_size: u32,

    /// Minimum number of connections to keep in the pool.
    /// Pre-warms connections to reduce latency on first requests.
    /// Defaults to pool_size / 4 if not set.
    #[serde(default)]
    pub min_connections: Option<u32>,

    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,

    /// Idle timeout in milliseconds.
    /// Connections idle longer than this will be closed.
    pub idle_timeout_ms: Option<u64>,

    /// Maximum lifetime of a connection in seconds.
    /// Connections older than this will be recycled.
    /// Defaults to 1800 (30 minutes) if not set.
    #[serde(default)]
    pub max_lifetime_secs: Option<u64>,

    /// Whether to run migrations on startup.
    pub run_migrations: bool,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            url: "postgres://localhost/octofhir".into(),
            // 20 connections is a good default for production workloads
            // with moderate concurrency. Adjust based on PostgreSQL max_connections
            // and expected concurrent request load.
            pool_size: 20,
            min_connections: None,
            connect_timeout_ms: 5000,
            idle_timeout_ms: Some(300_000),
            max_lifetime_secs: None,
            run_migrations: true,
        }
    }
}

impl PostgresConfig {
    /// Creates a new configuration with the given URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Sets the pool size.
    #[must_use]
    pub fn with_pool_size(mut self, size: u32) -> Self {
        self.pool_size = size;
        self
    }

    /// Sets the connection timeout.
    #[must_use]
    pub fn with_connect_timeout_ms(mut self, timeout: u64) -> Self {
        self.connect_timeout_ms = timeout;
        self
    }

    /// Sets the idle timeout.
    #[must_use]
    pub fn with_idle_timeout_ms(mut self, timeout: Option<u64>) -> Self {
        self.idle_timeout_ms = timeout;
        self
    }

    /// Sets the minimum number of connections to keep in the pool.
    #[must_use]
    pub fn with_min_connections(mut self, min: Option<u32>) -> Self {
        self.min_connections = min;
        self
    }

    /// Sets the maximum lifetime of a connection in seconds.
    #[must_use]
    pub fn with_max_lifetime_secs(mut self, lifetime: Option<u64>) -> Self {
        self.max_lifetime_secs = lifetime;
        self
    }

    /// Sets whether to run migrations on startup.
    #[must_use]
    pub fn with_run_migrations(mut self, run: bool) -> Self {
        self.run_migrations = run;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PostgresConfig::default();
        assert_eq!(config.url, "postgres://localhost/octofhir");
        assert_eq!(config.pool_size, 20);
        assert_eq!(config.connect_timeout_ms, 5000);
        assert_eq!(config.idle_timeout_ms, Some(300_000));
        assert!(config.run_migrations);
    }

    #[test]
    fn test_config_builder() {
        let config = PostgresConfig::new("postgres://test:test@localhost:5432/test")
            .with_pool_size(20)
            .with_connect_timeout_ms(10000)
            .with_idle_timeout_ms(None)
            .with_run_migrations(false);

        assert_eq!(config.url, "postgres://test:test@localhost:5432/test");
        assert_eq!(config.pool_size, 20);
        assert_eq!(config.connect_timeout_ms, 10000);
        assert_eq!(config.idle_timeout_ms, None);
        assert!(!config.run_migrations);
    }

    #[test]
    fn test_config_serialization() {
        let config = PostgresConfig::default();
        let json = serde_json::to_string(&config).expect("serialization failed");
        let deserialized: PostgresConfig =
            serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(config.url, deserialized.url);
        assert_eq!(config.pool_size, deserialized.pool_size);
    }
}
