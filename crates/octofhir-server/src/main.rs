use std::{env, path::PathBuf, sync::Arc};

// Use mimalloc as the global allocator for better performance
// Provides ~10-20% improvement in allocation-heavy workloads
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use octofhir_server::config::loader::load_config;
use octofhir_server::config_manager::ServerConfigManager;
use octofhir_server::{ServerBuilder, shutdown_tracing};
use tokio::sync::RwLock;

const STARTUP_BANNER: &str = r#"
       db         88888888ba  8b        d8  8b        d8  ,ad8888ba,    888b      88
      d88b        88      "8b  Y8,    ,8P    Y8,    ,8P  d8"'    `"8b   8888b     88
     d8'`8b       88      ,8P   Y8,  ,8P      `8b  d8'  d8'        `8b  88 `8b    88
    d8'  `8b      88aaaaaa8P'    "8aa8"         Y88P    88          88  88  `8b   88
   d8YaaaaY8b     88""""""8b,     `88'          d88b    88          88  88   `8b  88
  d8""""""""8b    88      `8b      88         ,8P  Y8,  Y8,        ,8P  88    `8b 88
 d8'        `8b   88      a8P      88        d8'    `8b  Y8a.    .a8P   88     `8888
d8'          `8b  88888888P"       88       8P        Y8  `"Y8888Y"'    88      `888
"#;

/// How the configuration path was determined.
#[derive(Debug, Clone, Copy)]
enum ConfigSource {
    /// From --config CLI argument
    CliArgument,
    /// From OCTOFHIR_CONFIG environment variable
    EnvironmentVariable,
    /// Default path (octofhir.toml)
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliArgument => write!(f, "CLI argument (--config)"),
            Self::EnvironmentVariable => write!(f, "environment variable (OCTOFHIR_CONFIG)"),
            Self::Default => write!(f, "default"),
        }
    }
}

#[tokio::main]
async fn main() {
    println!("{STARTUP_BANNER}");

    // Load .env file if present (before anything else)
    // This allows environment variables to be set from .env for local development
    if let Err(e) = dotenvy::dotenv() {
        // Not an error if .env doesn't exist - it's optional
        if !matches!(e, dotenvy::Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
        {
            eprintln!("Warning: Failed to load .env file: {e}");
        }
    }

    // Initialize tracing early with the default level
    octofhir_server::observability::init_tracing();

    // Initialize Prometheus metrics
    octofhir_server::metrics::init_metrics();

    // Parse config path from CLI, environment, or use default
    let (config_path, source) = resolve_config_path();

    // Load initial configuration
    let cfg = match load_config(Some(&config_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(2);
        }
    };

    tracing::info!(
        path = %config_path,
        source = %source,
        "Configuration loaded"
    );

    // Apply logging and OTEL settings
    octofhir_server::observability::apply_logging_level(&cfg.logging.level);
    octofhir_server::observability::apply_otel_config(&cfg.otel);

    // Initialize canonical registry
    let registry = match octofhir_server::canonical::init_from_config_async(&cfg).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Canonical manager initialization failed: {e}");
            std::process::exit(2);
        }
    };

    if let Ok(guard) = registry.read() {
        tracing::info!(
            fhir.version = %cfg.fhir.version,
            packages_loaded = %guard.list().len(),
            "Canonical registry initialized"
        );
    }
    octofhir_server::canonical::set_registry(registry);

    // Create shared config for hot-reload
    let shared_config = Arc::new(RwLock::new(cfg.clone()));

    // Initialize unified configuration manager (required)
    let config_manager = match init_config_manager(&config_path).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Configuration manager initialization failed: {e}");
            std::process::exit(2);
        }
    };

    // Start config watcher
    config_manager.start_watching(shared_config.clone()).await;
    tracing::info!("Hot-reload enabled");

    // Build and run server
    let server = match ServerBuilder::new(config_manager.inner_arc())
        .with_config(cfg)
        .build()
        .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Server initialization failed: {e}");
            std::process::exit(2);
        }
    };

    if let Err(err) = server.run().await {
        eprintln!("Server error: {err}");
    }

    shutdown_tracing();
}

/// Resolve the configuration file path.
///
/// Priority order:
/// 1. CLI argument: --config <path>
/// 2. Environment variable: OCTOFHIR_CONFIG
/// 3. Default: octofhir.toml
fn resolve_config_path() -> (String, ConfigSource) {
    // 1. Check CLI: --config <path>
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config"
            && let Some(path) = args.next() {
                return (path, ConfigSource::CliArgument);
            }
    }

    // 2. Check environment variable
    if let Ok(path) = env::var("OCTOFHIR_CONFIG")
        && !path.is_empty() {
            return (path, ConfigSource::EnvironmentVariable);
        }

    // 3. Default to octofhir.toml
    ("octofhir.toml".to_string(), ConfigSource::Default)
}

/// Initialize the unified configuration manager.
async fn init_config_manager(
    config_path: &str,
) -> Result<ServerConfigManager, octofhir_config::ConfigError> {
    let path_buf = PathBuf::from(config_path);

    let mut builder = ServerConfigManager::builder();

    if path_buf.exists() {
        builder = builder.with_file(path_buf);
    }

    // Note: Database source can be added here when pool is available
    // builder = builder.with_database(pool);

    builder.build().await
}
