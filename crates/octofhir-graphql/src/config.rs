//! GraphQL configuration.
//!
//! This module provides configuration options for the GraphQL layer.
//! Configuration can be specified in `octofhir.toml` under the `[graphql]` section.
//!
//! # Example Configuration
//!
//! ```toml
//! [graphql]
//! enabled = true
//! max_depth = 15
//! max_complexity = 500
//! introspection = true
//! ```

use serde::{Deserialize, Serialize};

/// GraphQL API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLConfig {
    /// Enable GraphQL API endpoints.
    /// Default: false (opt-in feature)
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum query depth allowed.
    /// Limits nesting of fields to prevent denial-of-service attacks.
    /// Default: 15
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Maximum query complexity allowed.
    /// Each field has a complexity cost; complex queries are rejected.
    /// Default: 500
    #[serde(default = "default_max_complexity")]
    pub max_complexity: usize,

    /// Enable GraphQL introspection queries.
    /// Allows clients to query the schema itself.
    /// Should be disabled in production for security.
    /// Default: true (development-friendly)
    #[serde(default = "default_introspection")]
    pub introspection: bool,

    /// Batch query support.
    /// Allow multiple operations in a single request.
    /// Default: false
    #[serde(default = "default_batching")]
    pub batching: bool,

    /// Maximum batch size for batched queries.
    /// Only applies if batching is enabled.
    /// Default: 10
    #[serde(default = "default_max_batch_size")]
    pub max_batch_size: usize,
}

fn default_enabled() -> bool {
    false
}

fn default_max_depth() -> usize {
    15
}

fn default_max_complexity() -> usize {
    500
}

fn default_introspection() -> bool {
    true
}

fn default_batching() -> bool {
    false
}

fn default_max_batch_size() -> usize {
    10
}

impl Default for GraphQLConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_depth: default_max_depth(),
            max_complexity: default_max_complexity(),
            introspection: default_introspection(),
            batching: default_batching(),
            max_batch_size: default_max_batch_size(),
        }
    }
}

impl GraphQLConfig {
    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration values are invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_depth == 0 {
            return Err("graphql.max_depth must be > 0".into());
        }
        if self.max_complexity == 0 {
            return Err("graphql.max_complexity must be > 0".into());
        }
        if self.batching && self.max_batch_size == 0 {
            return Err("graphql.max_batch_size must be > 0 when batching is enabled".into());
        }
        Ok(())
    }

    /// Converts this config to a SchemaBuilderConfig.
    #[must_use]
    pub fn to_schema_builder_config(&self) -> crate::SchemaBuilderConfig {
        crate::SchemaBuilderConfig {
            max_depth: self.max_depth,
            max_complexity: self.max_complexity,
            introspection_enabled: self.introspection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GraphQLConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_depth, 15);
        assert_eq!(config.max_complexity, 500);
        assert!(config.introspection);
        assert!(!config.batching);
        assert_eq!(config.max_batch_size, 10);
    }

    #[test]
    fn test_valid_config() {
        let config = GraphQLConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_max_depth() {
        let mut config = GraphQLConfig::default();
        config.max_depth = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_max_complexity() {
        let mut config = GraphQLConfig::default();
        config.max_complexity = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_batch_size() {
        let mut config = GraphQLConfig::default();
        config.batching = true;
        config.max_batch_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_deserialize_from_toml() {
        let toml = r#"
            enabled = true
            max_depth = 20
            max_complexity = 1000
            introspection = false
        "#;

        let config: GraphQLConfig = toml::from_str(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.max_depth, 20);
        assert_eq!(config.max_complexity, 1000);
        assert!(!config.introspection);
    }
}
