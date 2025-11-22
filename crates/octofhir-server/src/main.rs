use std::{
    env,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use octofhir_server::config::{loader::load_config, shared};
use octofhir_server::{ServerBuilder, shutdown_tracing};

#[tokio::main]
async fn main() {
    // Initialize tracing early with the default level so we can log during a config load
    octofhir_server::observability::init_tracing();

    // Basic CLI: --config <path>
    let mut args = env::args().skip(1);
    let mut config_path: Option<String> = None;
    while let Some(arg) = args.next() {
        if arg == "--config"
            && let Some(p) = args.next()
        {
            config_path = Some(p);
        }
    }
    if config_path.is_none()
        && let Ok(p) = env::var("OCTOFHIR_CONFIG")
        && !p.is_empty()
    {
        config_path = Some(p);
    }
    // Default to root-level octofhir.toml when not provided
    if config_path.is_none() {
        config_path = Some("octofhir.toml".to_string());
    }

    let cfg = match load_config(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };

    // Log that configuration was read successfully
    tracing::info!(path = %config_path.as_deref().unwrap_or("octofhir.toml"), "configuration loaded successfully");

    // Apply logging level from configuration dynamically
    octofhir_server::observability::apply_logging_level(&cfg.logging.level);
    // Apply OTEL config (Phase 7 implements tracer initialization)
    octofhir_server::observability::apply_otel_config(&cfg.otel);

    // Initialize the canonical registry from configuration (Phase 8) asynchronously
    let registry = match octofhir_server::canonical::init_from_config_async(&cfg).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("canonical manager init failed: {e}");
            std::process::exit(2);
        }
    };
    if let Ok(guard) = registry.read() {
        tracing::info!(fhir.version = %cfg.fhir.version, packages_loaded = %guard.list().len(), "canonical registry initialized");
    }
    octofhir_server::canonical::set_registry(registry);

    // Initialize shared config for hot-reload
    let shared_cfg = Arc::new(RwLock::new(cfg.clone()));
    shared::set_shared(shared_cfg.clone());

    // Start the config watcher if a path is provided
    let rt_handle = tokio::runtime::Handle::current();
    let _watcher_guard = config_path.as_ref().and_then(|p| {
        let path = PathBuf::from(p);
        octofhir_server::config_watch::start_config_watcher(path, shared_cfg, rt_handle)
    });

    // Build server with the appropriate storage backend
    let server = match ServerBuilder::new().with_config(cfg).build().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to initialize server: {e}");
            std::process::exit(2);
        }
    };

    if let Err(err) = server.run().await {
        eprintln!("server error: {err}");
    }

    shutdown_tracing();
}
