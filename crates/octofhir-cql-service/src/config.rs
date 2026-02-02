//! CQL service configuration

use serde::{Deserialize, Serialize};

/// Configuration for the CQL evaluation service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CqlConfig {
    /// Enable CQL service
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum resources per retrieve (prevent memory exhaustion)
    #[serde(default = "default_max_retrieve_size")]
    pub max_retrieve_size: usize,

    /// Compiled library cache capacity
    #[serde(default = "default_cache_capacity")]
    pub cache_capacity: usize,

    /// Library compilation timeout (ms)
    #[serde(default = "default_compile_timeout")]
    pub compile_timeout_ms: u64,

    /// Expression evaluation timeout (ms)
    #[serde(default = "default_evaluation_timeout")]
    pub evaluation_timeout_ms: u64,
}

impl Default for CqlConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_retrieve_size: default_max_retrieve_size(),
            cache_capacity: default_cache_capacity(),
            compile_timeout_ms: default_compile_timeout(),
            evaluation_timeout_ms: default_evaluation_timeout(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_max_retrieve_size() -> usize {
    10_000
}

fn default_cache_capacity() -> usize {
    1_000
}

fn default_compile_timeout() -> u64 {
    5_000
}

fn default_evaluation_timeout() -> u64 {
    30_000
}
