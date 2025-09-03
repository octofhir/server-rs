pub mod config;
pub mod handlers;
pub mod middleware;
pub mod observability;
pub mod server;

pub use config::{AppConfig, OtelConfig, ServerConfig};
pub use observability::{init_tracing, shutdown_tracing};
pub use server::{OctofhirServer, ServerBuilder, build_app};
