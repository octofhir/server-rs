// Basic tracing initialization with configurable and reloadable log level.
use std::sync::OnceLock;
use tracing_subscriber::{fmt, EnvFilter, prelude::*, reload};
use crate::config::OtelConfig;

static LOG_RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> = OnceLock::new();

pub fn init_tracing() {
    init_tracing_with_level("info");
}

pub fn init_tracing_with_level(level: &str) {
    // Prefer RUST_LOG from env, otherwise use provided level string.
    let base_filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|_| EnvFilter::try_from_default_env().ok())
        .unwrap_or_else(|| EnvFilter::new(level));

    let (reload_layer, handle) = reload::Layer::new(base_filter);
    let _ = LOG_RELOAD_HANDLE.set(handle);

    let _ = tracing_subscriber::registry()
        .with(reload_layer)
        .with(fmt::layer())
        .try_init();
}

/// Apply a new logging level at runtime if reload handle is configured.
pub fn apply_logging_level(level: &str) {
    if let Some(handle) = LOG_RELOAD_HANDLE.get() {
        let _ = handle.modify(|f| {
            *f = EnvFilter::new(level);
        });
    }
}

/// Apply OTEL configuration change. Placeholder: logs intent; OTEL pipeline is implemented in Phase 7.
pub fn apply_otel_config(otel: &OtelConfig) {
    if otel.enabled {
        let endpoint = otel.endpoint.as_deref().unwrap_or("");
        if endpoint.is_empty() {
            tracing::warn!("OTEL enabled but endpoint is empty; ignoring");
        } else {
            tracing::info!(endpoint, sample_ratio = ?otel.sample_ratio, "OTEL config applied (restart tracer pending in Phase 7)");
        }
    } else {
        tracing::info!("OTEL disabled");
    }
}

pub fn shutdown_tracing() {
    // No-op for now; add OTEL shutdown when enabled in Phase 7
}
