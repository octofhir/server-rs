//! Unified Configuration Management System for OctoFHIR
//!
//! This crate provides a comprehensive configuration management system that:
//! - Watches multiple configuration sources (file, database, API)
//! - Merges configurations with priority ordering
//! - Broadcasts configuration changes via event bus
//! - Supports feature flags with rollout capabilities
//! - Encrypts secret values at rest
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    ConfigurationManager                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │ FileWatcher │  │ APIHandler  │  │  DBListener │              │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │
//! │         └────────────────┴────────────────┘                      │
//! │                          │                                       │
//! │                    ┌─────▼─────┐                                 │
//! │                    │  Merger   │                                 │
//! │                    └─────┬─────┘                                 │
//! │                          │                                       │
//! │                    Event Bus                                     │
//! └──────────────────────────┬──────────────────────────────────────┘
//!                            │
//!         ┌──────────────────┼──────────────────┐
//!         ▼                  ▼                  ▼
//!    Components         Components         Components
//! ```

pub mod events;
pub mod feature_flags;
pub mod manager;
pub mod merger;
pub mod secrets;
pub mod sources;
pub mod storage;

// Re-export main types
pub use events::{ConfigCategory, ConfigChangeEvent, ConfigSource as ConfigSourceType};
pub use feature_flags::{FeatureContext, FeatureFlag, FeatureFlags};
pub use manager::{ConfigurationManager, ConfigurationManagerBuilder};
pub use merger::{MergedConfig, PartialConfig};
pub use secrets::{SecretValue, Secrets};
pub use sources::ConfigSource;

/// Error types for configuration operations
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Watcher error: {0}")]
    Watcher(String),

    #[error("Source error: {source}")]
    Source {
        source_name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl ConfigError {
    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn database(msg: impl Into<String>) -> Self {
        Self::Database(msg.into())
    }

    pub fn encryption(msg: impl Into<String>) -> Self {
        Self::Encryption(msg.into())
    }

    pub fn watcher(msg: impl Into<String>) -> Self {
        Self::Watcher(msg.into())
    }
}

/// Result type for configuration operations
pub type Result<T> = std::result::Result<T, ConfigError>;
