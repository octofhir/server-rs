// Basic tracing initialization. Can be extended with OpenTelemetry later.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt::try_init();
}

pub fn shutdown_tracing() {
    // No-op for now; add OTEL shutdown when enabled
}
