//! Configuration merging with priority ordering
//!
//! Priority order (lowest to highest):
//! 1. Defaults - Hardcoded sane defaults
//! 2. File config - From octofhir.toml
//! 3. Database config - Persisted overrides
//! 4. Environment variables - OCTOFHIR__* pattern
//! 5. API/runtime - Ephemeral overrides

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::ConfigError;
use crate::events::ConfigSource;
use crate::feature_flags::FeatureFlags;

/// Priority levels for configuration sources
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Default = 0,
    File = 10,
    Database = 20,
    Environment = 30,
    Api = 40,
}

impl From<ConfigSource> for Priority {
    fn from(source: ConfigSource) -> Self {
        match source {
            ConfigSource::Default => Priority::Default,
            ConfigSource::File => Priority::File,
            ConfigSource::Database => Priority::Database,
            ConfigSource::Environment => Priority::Environment,
            ConfigSource::Api => Priority::Api,
        }
    }
}

/// Partial configuration that may have some fields set
///
/// Used for merging configurations from different sources.
/// Each field is Option<Value> to allow partial overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartialConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otel: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminology: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redis: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Value>,
}

impl PartialConfig {
    /// Create an empty partial config
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse from TOML string
    pub fn from_toml(toml_str: &str) -> Result<Self, ConfigError> {
        toml::from_str(toml_str).map_err(|e| ConfigError::parse(format!("TOML parse error: {e}")))
    }

    /// Parse from JSON value
    pub fn from_json(value: Value) -> Result<Self, ConfigError> {
        serde_json::from_value(value)
            .map_err(|e| ConfigError::parse(format!("JSON parse error: {e}")))
    }

    /// Check if a category has a value
    pub fn has_category(&self, category: &str) -> bool {
        match category {
            "fhir" => self.fhir.is_some(),
            "server" => self.server.is_some(),
            "storage" => self.storage.is_some(),
            "search" => self.search.is_some(),
            "logging" => self.logging.is_some(),
            "otel" => self.otel.is_some(),
            "packages" => self.packages.is_some(),
            "auth" => self.auth.is_some(),
            "terminology" => self.terminology.is_some(),
            "validation" => self.validation.is_some(),
            "redis" => self.redis.is_some(),
            "cache" => self.cache.is_some(),
            "features" => self.features.is_some(),
            _ => false,
        }
    }

    /// Get a category value
    pub fn get_category(&self, category: &str) -> Option<&Value> {
        match category {
            "fhir" => self.fhir.as_ref(),
            "server" => self.server.as_ref(),
            "storage" => self.storage.as_ref(),
            "search" => self.search.as_ref(),
            "logging" => self.logging.as_ref(),
            "otel" => self.otel.as_ref(),
            "packages" => self.packages.as_ref(),
            "auth" => self.auth.as_ref(),
            "terminology" => self.terminology.as_ref(),
            "validation" => self.validation.as_ref(),
            "redis" => self.redis.as_ref(),
            "cache" => self.cache.as_ref(),
            "features" => self.features.as_ref(),
            _ => None,
        }
    }

    /// Set a category value
    pub fn set_category(&mut self, category: &str, value: Value) {
        match category {
            "fhir" => self.fhir = Some(value),
            "server" => self.server = Some(value),
            "storage" => self.storage = Some(value),
            "search" => self.search = Some(value),
            "logging" => self.logging = Some(value),
            "otel" => self.otel = Some(value),
            "packages" => self.packages = Some(value),
            "auth" => self.auth = Some(value),
            "terminology" => self.terminology = Some(value),
            "validation" => self.validation = Some(value),
            "redis" => self.redis = Some(value),
            "cache" => self.cache = Some(value),
            "features" => self.features = Some(value),
            _ => {}
        }
    }
}

/// Tracked value with its source
#[derive(Debug, Clone)]
pub struct TrackedValue {
    pub value: Value,
    pub source: ConfigSource,
    pub priority: Priority,
}

/// Merged configuration with tracking of value sources
#[derive(Debug, Clone)]
pub struct MergedConfig {
    /// Merged configuration as JSON
    config: Value,
    /// Track which source each top-level key came from
    sources: HashMap<String, ConfigSource>,
    /// Feature flags (special handling)
    feature_flags: FeatureFlags,
}

impl MergedConfig {
    /// Create with default values
    pub fn defaults() -> Self {
        // Default configuration structure
        let config = serde_json::json!({
            "fhir": {
                "version": "R4"
            },
            "server": {
                "host": "0.0.0.0",
                "port": 8080,
                "read_timeout_ms": 15000,
                "write_timeout_ms": 15000,
                "body_limit_bytes": 1048576
            },
            "storage": {
                "postgres": {
                    "host": "localhost",
                    "port": 5432,
                    "user": "postgres",
                    "database": "octofhir",
                    "pool_size": 10,
                    "connect_timeout_ms": 5000
                }
            },
            "search": {
                "default_count": 10,
                "max_count": 100
            },
            "logging": {
                "level": "info"
            },
            "otel": {
                "enabled": false
            },
            "packages": {
                "load": []
            },
            "auth": {
                "enabled": false
            },
            "terminology": {
                "enabled": true,
                "server_url": "https://tx.fhir.org/r4"
            },
            "validation": {
                "allow_skip_validation": false
            },
            "redis": {
                "enabled": false,
                "url": "redis://localhost:6379",
                "pool_size": 10,
                "timeout_ms": 5000
            },
            "cache": {
                "terminology_ttl_secs": 3600,
                "local_cache_max_entries": 10000
            }
        });

        let mut sources = HashMap::new();
        for key in [
            "fhir",
            "server",
            "storage",
            "search",
            "logging",
            "otel",
            "packages",
            "auth",
            "terminology",
            "validation",
            "redis",
            "cache",
        ] {
            sources.insert(key.to_string(), ConfigSource::Default);
        }

        Self {
            config,
            sources,
            feature_flags: FeatureFlags::with_defaults(),
        }
    }

    /// Merge a partial config with given priority
    pub fn merge(&mut self, partial: PartialConfig, source: ConfigSource) {
        let priority = Priority::from(source);

        for category in [
            "fhir",
            "server",
            "storage",
            "search",
            "logging",
            "otel",
            "packages",
            "auth",
            "terminology",
            "validation",
            "redis",
            "cache",
        ] {
            if let Some(new_value) = partial.get_category(category) {
                // Check if we should override based on priority
                let should_override = self
                    .sources
                    .get(category)
                    .map(|&existing_source| Priority::from(existing_source) <= priority)
                    .unwrap_or(true);

                if should_override {
                    // Deep merge the JSON objects
                    if let Some(existing) = self.config.get_mut(category) {
                        deep_merge(existing, new_value.clone());
                    } else {
                        self.config[category] = new_value.clone();
                    }
                    self.sources.insert(category.to_string(), source);
                }
            }
        }

        // Handle feature flags specially
        if let Some(features_value) = partial.features {
            if let Ok(flags) = serde_json::from_value::<FeatureFlags>(features_value) {
                self.feature_flags.merge(flags);
            }
        }
    }

    /// Get the merged configuration as JSON
    pub fn as_json(&self) -> &Value {
        &self.config
    }

    /// Get a specific category
    pub fn get_category(&self, category: &str) -> Option<&Value> {
        self.config.get(category)
    }

    /// Get the source for a category
    pub fn get_source(&self, category: &str) -> Option<ConfigSource> {
        self.sources.get(category).copied()
    }

    /// Get the feature flags
    pub fn feature_flags(&self) -> &FeatureFlags {
        &self.feature_flags
    }

    /// Get mutable feature flags
    pub fn feature_flags_mut(&mut self) -> &mut FeatureFlags {
        &mut self.feature_flags
    }

    /// Get mutable access to the underlying JSON config
    pub fn config_mut(&mut self) -> &mut Value {
        &mut self.config
    }

    /// Get immutable access to the underlying JSON config
    pub fn config(&self) -> &Value {
        &self.config
    }

    /// Validate the merged configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Server validation
        if let Some(server) = self.config.get("server") {
            if let Some(port) = server.get("port").and_then(|v| v.as_u64()) {
                if port == 0 {
                    return Err(ConfigError::validation("server.port must be > 0"));
                }
            }
        }

        // Search validation
        if let Some(search) = self.config.get("search") {
            let default_count = search
                .get("default_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(10);
            let max_count = search
                .get("max_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(100);

            if default_count == 0 {
                return Err(ConfigError::validation("search.default_count must be > 0"));
            }
            if max_count == 0 {
                return Err(ConfigError::validation("search.max_count must be > 0"));
            }
            if default_count > max_count {
                return Err(ConfigError::validation(
                    "search.default_count must be <= search.max_count",
                ));
            }
        }

        // Storage validation
        if let Some(storage) = self.config.get("storage") {
            if storage.get("postgres").is_none() {
                return Err(ConfigError::validation(
                    "storage.postgres config is required",
                ));
            }
        }

        Ok(())
    }

    /// Convert to a specific config type
    pub fn deserialize<T: for<'de> Deserialize<'de>>(&self) -> Result<T, ConfigError> {
        serde_json::from_value(self.config.clone())
            .map_err(|e| ConfigError::parse(format!("Failed to deserialize config: {e}")))
    }
}

/// Deep merge two JSON values (right takes precedence for conflicts)
fn deep_merge(left: &mut Value, right: Value) {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            for (key, right_value) in right_map {
                if let Some(left_value) = left_map.get_mut(&key) {
                    deep_merge(left_value, right_value);
                } else {
                    left_map.insert(key, right_value);
                }
            }
        }
        (left, right) => {
            *left = right;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let config = MergedConfig::defaults();
        assert!(config.get_category("server").is_some());
        assert!(config.get_category("search").is_some());
        assert_eq!(config.get_source("server"), Some(ConfigSource::Default));
    }

    #[test]
    fn test_merge_override() {
        let mut config = MergedConfig::defaults();

        let partial = PartialConfig {
            server: Some(serde_json::json!({
                "port": 9090
            })),
            ..Default::default()
        };

        config.merge(partial, ConfigSource::File);

        let server = config.get_category("server").unwrap();
        assert_eq!(server.get("port").unwrap().as_u64(), Some(9090));
        // host should still have default value
        assert_eq!(server.get("host").unwrap().as_str(), Some("0.0.0.0"));
        assert_eq!(config.get_source("server"), Some(ConfigSource::File));
    }

    #[test]
    fn test_priority_ordering() {
        let mut config = MergedConfig::defaults();

        // File config
        config.merge(
            PartialConfig {
                server: Some(serde_json::json!({ "port": 8081 })),
                ..Default::default()
            },
            ConfigSource::File,
        );

        // Database config (higher priority)
        config.merge(
            PartialConfig {
                server: Some(serde_json::json!({ "port": 8082 })),
                ..Default::default()
            },
            ConfigSource::Database,
        );

        // File again (lower priority, should not override)
        config.merge(
            PartialConfig {
                server: Some(serde_json::json!({ "port": 8083 })),
                ..Default::default()
            },
            ConfigSource::File,
        );

        let port = config
            .get_category("server")
            .unwrap()
            .get("port")
            .unwrap()
            .as_u64();
        assert_eq!(port, Some(8082));
    }

    #[test]
    fn test_deep_merge() {
        let mut left = serde_json::json!({
            "a": {
                "b": 1,
                "c": 2
            }
        });

        let right = serde_json::json!({
            "a": {
                "c": 3,
                "d": 4
            }
        });

        deep_merge(&mut left, right);

        assert_eq!(left["a"]["b"], 1);
        assert_eq!(left["a"]["c"], 3);
        assert_eq!(left["a"]["d"], 4);
    }

    #[test]
    fn test_validation() {
        let config = MergedConfig::defaults();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_partial_from_toml() {
        let toml_str = r#"
[server]
port = 9090

[search]
default_count = 20
"#;

        let partial = PartialConfig::from_toml(toml_str).unwrap();
        assert!(partial.server.is_some());
        assert!(partial.search.is_some());
        assert!(partial.auth.is_none());
    }
}
