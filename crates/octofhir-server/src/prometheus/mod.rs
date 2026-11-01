//! Metrics dashboards: the console reads this deployment's metrics from Prometheus (never from
//! `/metrics` directly, which behind a load balancer answers from a random instance), and exports
//! its dashboards to Grafana.

pub mod client;
pub mod dashboards;
pub mod error;
pub mod explain;
pub mod grafana;
pub mod handlers;
pub mod promql;

pub use client::PrometheusClient;
pub use error::MetricsError;
