//! Configuration change events and event bus types

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Source of a configuration change
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    /// Configuration loaded from file
    File,
    /// Configuration loaded from database
    Database,
    /// Configuration set via API
    Api,
    /// Configuration from environment variables
    Environment,
    /// Default values
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Database => write!(f, "database"),
            Self::Api => write!(f, "api"),
            Self::Environment => write!(f, "environment"),
            Self::Default => write!(f, "default"),
        }
    }
}

/// Category of configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigCategory {
    /// Server configuration (host, port, timeouts)
    Server,
    /// Search configuration (default/max count, registry)
    Search,
    /// Terminology service configuration
    Terminology,
    /// Authentication and authorization
    Auth,
    /// Cache configuration (TTLs, sizes)
    Cache,
    /// Feature flags
    Features,
    /// Logging configuration
    Logging,
    /// OpenTelemetry configuration
    Otel,
    /// Storage/database configuration
    Storage,
    /// Redis configuration
    Redis,
    /// Validation settings
    Validation,
    /// FHIR settings
    Fhir,
    /// Packages configuration
    Packages,
    /// DB Console configuration (SQL execution, LSP)
    DbConsole,
}

impl ConfigCategory {
    /// Returns all categories
    pub fn all() -> &'static [ConfigCategory] {
        &[
            Self::Server,
            Self::Search,
            Self::Terminology,
            Self::Auth,
            Self::Cache,
            Self::Features,
            Self::Logging,
            Self::Otel,
            Self::Storage,
            Self::Redis,
            Self::Validation,
            Self::Fhir,
            Self::Packages,
            Self::DbConsole,
        ]
    }

    /// Parse category from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "server" => Some(Self::Server),
            "search" => Some(Self::Search),
            "terminology" => Some(Self::Terminology),
            "auth" => Some(Self::Auth),
            "cache" => Some(Self::Cache),
            "features" => Some(Self::Features),
            "logging" => Some(Self::Logging),
            "otel" => Some(Self::Otel),
            "storage" => Some(Self::Storage),
            "redis" => Some(Self::Redis),
            "validation" => Some(Self::Validation),
            "fhir" => Some(Self::Fhir),
            "packages" => Some(Self::Packages),
            "db_console" | "dbconsole" => Some(Self::DbConsole),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConfigCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server => write!(f, "server"),
            Self::Search => write!(f, "search"),
            Self::Terminology => write!(f, "terminology"),
            Self::Auth => write!(f, "auth"),
            Self::Cache => write!(f, "cache"),
            Self::Features => write!(f, "features"),
            Self::Logging => write!(f, "logging"),
            Self::Otel => write!(f, "otel"),
            Self::Storage => write!(f, "storage"),
            Self::Redis => write!(f, "redis"),
            Self::Validation => write!(f, "validation"),
            Self::Fhir => write!(f, "fhir"),
            Self::Packages => write!(f, "packages"),
            Self::DbConsole => write!(f, "db_console"),
        }
    }
}

/// Operation type for configuration changes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigOperation {
    /// Configuration value was created/set
    Set,
    /// Configuration value was updated
    Update,
    /// Configuration value was deleted/reset to default
    Delete,
    /// Full configuration reload
    Reload,
}

impl std::fmt::Display for ConfigOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Set => write!(f, "set"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::Reload => write!(f, "reload"),
        }
    }
}

/// Event representing a configuration change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeEvent {
    /// Source of the change
    pub source: ConfigSource,
    /// Category of configuration that changed
    pub category: ConfigCategory,
    /// Specific key that changed (if applicable)
    pub key: Option<String>,
    /// Operation type
    pub operation: ConfigOperation,
    /// Timestamp of the change
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    /// Optional new value (for debugging, not included for secrets)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<serde_json::Value>,
}

impl ConfigChangeEvent {
    /// Create a new configuration change event
    pub fn new(source: ConfigSource, category: ConfigCategory, operation: ConfigOperation) -> Self {
        Self {
            source,
            category,
            key: None,
            operation,
            timestamp: OffsetDateTime::now_utc(),
            new_value: None,
        }
    }

    /// Create event for a specific key change
    pub fn with_key(
        source: ConfigSource,
        category: ConfigCategory,
        key: impl Into<String>,
        operation: ConfigOperation,
    ) -> Self {
        Self {
            source,
            category,
            key: Some(key.into()),
            operation,
            timestamp: OffsetDateTime::now_utc(),
            new_value: None,
        }
    }

    /// Add the new value to the event (for non-secret values)
    pub fn with_value(mut self, value: serde_json::Value) -> Self {
        self.new_value = Some(value);
        self
    }

    /// Create a reload event for all categories from file
    pub fn file_reload() -> Self {
        Self::new(
            ConfigSource::File,
            ConfigCategory::Server,
            ConfigOperation::Reload,
        )
    }

    /// Create a reload event from database
    pub fn database_reload(category: ConfigCategory) -> Self {
        Self::new(ConfigSource::Database, category, ConfigOperation::Reload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_source_display() {
        assert_eq!(ConfigSource::File.to_string(), "file");
        assert_eq!(ConfigSource::Database.to_string(), "database");
        assert_eq!(ConfigSource::Api.to_string(), "api");
    }

    #[test]
    fn test_config_category_from_str() {
        assert_eq!(
            ConfigCategory::from_str("server"),
            Some(ConfigCategory::Server)
        );
        assert_eq!(
            ConfigCategory::from_str("SEARCH"),
            Some(ConfigCategory::Search)
        );
        assert_eq!(ConfigCategory::from_str("unknown"), None);
    }

    #[test]
    fn test_config_change_event_creation() {
        let event = ConfigChangeEvent::new(
            ConfigSource::File,
            ConfigCategory::Search,
            ConfigOperation::Update,
        );
        assert_eq!(event.source, ConfigSource::File);
        assert_eq!(event.category, ConfigCategory::Search);
        assert!(event.key.is_none());
    }

    #[test]
    fn test_config_change_event_with_key() {
        let event = ConfigChangeEvent::with_key(
            ConfigSource::Api,
            ConfigCategory::Features,
            "search.optimization.enabled",
            ConfigOperation::Set,
        );
        assert_eq!(event.key, Some("search.optimization.enabled".to_string()));
    }
}
