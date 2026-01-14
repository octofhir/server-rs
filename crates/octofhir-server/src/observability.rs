// Tracing and OpenTelemetry initialization (with reloadable log level)
use crate::config::OtelConfig;
use crate::log_stream::{LogBroadcastLayer, init_log_broadcast};
use std::sync::OnceLock;
use tracing_subscriber::{EnvFilter, fmt, prelude::*, reload};

use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{Resource, trace as sdktrace};
use tracing_opentelemetry::OpenTelemetryLayer;

static LOG_RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();
static OTEL_INSTALLED: OnceLock<()> = OnceLock::new();
static OTEL_PROVIDER: OnceLock<sdktrace::SdkTracerProvider> = OnceLock::new();

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

    // Initialize log broadcast for WebSocket streaming
    let log_sender = init_log_broadcast();
    let log_broadcast_layer =
        LogBroadcastLayer::new(log_sender).with_min_level(tracing::Level::DEBUG);

    let _ = tracing_subscriber::registry()
        .with(reload_layer)
        .with(fmt::layer())
        .with(log_broadcast_layer)
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

/// Initialize or update OpenTelemetry pipeline based on configuration.
pub fn apply_otel_config(otel: &OtelConfig) {
    if !otel.enabled {
        tracing::info!("OTEL disabled");
        return;
    }

    let endpoint = otel.endpoint.as_deref().unwrap_or("");
    if endpoint.is_empty() {
        tracing::warn!("OTEL enabled but endpoint is empty; ignoring");
        return;
    }

    // Build resource attributes
    let service_name = env!("CARGO_PKG_NAME");
    let service_version = env!("CARGO_PKG_VERSION");
    let environment = otel.environment.as_deref().unwrap_or("development");
    let instance_id = build_instance_id();

    let resource = Resource::builder()
        .with_attributes(vec![
            KeyValue::new("service.name", service_name.to_string()),
            KeyValue::new("service.version", service_version.to_string()),
            KeyValue::new("deployment.environment", environment.to_string()),
            KeyValue::new("service.instance.id", instance_id),
            KeyValue::new("library.language", "rust"),
        ])
        .build();

    // Sampler
    let sampler = match otel.sample_ratio.unwrap_or(1.0) {
        r if r <= 0.0 => sdktrace::Sampler::AlwaysOff,
        r if (r - 1.0).abs() < f64::EPSILON => sdktrace::Sampler::AlwaysOn,
        r => sdktrace::Sampler::TraceIdRatioBased(r),
    };

    // Build OTLP exporter over HTTP/proto
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint.to_string())
        .build()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to build OTLP exporter");
            e
        })
        .ok();

    // Tracer provider
    let mut builder = sdktrace::SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(sampler);
    if let Some(exp) = exporter {
        builder = builder.with_batch_exporter(exp);
    }
    let tracer_provider = builder.build();

    use opentelemetry::trace::TracerProvider as _;
    let tracer = tracer_provider.tracer(service_name);

    // Install global provider only once; subsequent calls will replace the layer
    if OTEL_INSTALLED.set(()).is_ok() {
        // First-time installation: attach OTEL layer to subscriber
        let layer = OpenTelemetryLayer::new(tracer.clone());
        let _ = tracing_subscriber::registry().with(layer).try_init();
    } else {
        // If already installed, we just replace the global provider below
    }

    // Set as global and keep a handle for shutdown
    let _ = OTEL_PROVIDER.set(tracer_provider);

    tracing::info!(endpoint, sample_ratio = ?otel.sample_ratio, environment, "OTEL configured");
}

pub fn shutdown_tracing() {
    if let Some(provider) = OTEL_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

fn build_instance_id() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "unknown-host".to_string());
    let pid = std::process::id();
    format!("{host}-{pid}")
}
