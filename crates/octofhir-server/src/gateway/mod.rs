//! API Gateway functionality for dynamic routing based on App and CustomOperation resources.
//!
//! This module provides a flexible gateway system that allows defining custom API endpoints
//! through FHIR resources stored in the database. It supports multiple operation types:
//!
//! - **Proxy**: Forward requests to external services
//! - **SQL**: Execute SQL queries and return results as JSON
//! - **FHIRPath**: Evaluate FHIRPath expressions on request data
//! - **Handler**: Invoke custom Rust handlers
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │   Request   │
//! └──────┬──────┘
//!        │
//!        ▼
//! ┌─────────────────┐
//! │ GatewayRouter   │  (method:path -> CustomOperation)
//! └──────┬──────────┘
//!        │
//!        ├─▶ Proxy Handler    (forward to external URL)
//!        ├─▶ SQL Handler      (execute query)
//!        ├─▶ FHIRPath Handler (evaluate expression)
//!        └─▶ Custom Handler   (invoke registry)
//! ```
//!
//! # Example
//!
//! ```ignore
//! // Create an App
//! POST /App
//! {
//!   "resourceType": "App",
//!   "name": "External API",
//!   "basePath": "/api/v1/external",
//!   "active": true
//! }
//!
//! // Create a CustomOperation
//! POST /CustomOperation
//! {
//!   "resourceType": "CustomOperation",
//!   "app": { "reference": "App/123" },
//!   "path": "/users",
//!   "method": "GET",
//!   "type": "proxy",
//!   "active": true,
//!   "proxy": {
//!     "url": "https://jsonplaceholder.typicode.com/users",
//!     "timeout": 10
//!   }
//! }
//!
//! // Now requests to GET /api/v1/external/users will be proxied
//! ```

pub mod error;
pub mod fhirpath;
pub mod handler;
pub mod proxy;
pub mod reload;
pub mod router;
pub mod sql;
pub mod types;

pub use error::GatewayError;
pub use handler::HandlerRegistry;
pub use reload::{GatewayReloadBuilder, GatewayReloadListener};
pub use router::GatewayRouter;
pub use types::{App, CustomOperation, ProxyConfig, Reference, RouteKey};
