pub mod canonical;
pub mod config;
pub mod config_watch;
pub mod handlers;
pub mod mapping;
pub mod middleware;
pub mod observability;
pub mod server;
pub mod validation;

pub use config::{AppConfig, OtelConfig, ServerConfig};
pub use observability::{init_tracing, shutdown_tracing};
pub use server::{AppState, OctofhirServer, ServerBuilder, build_app};
