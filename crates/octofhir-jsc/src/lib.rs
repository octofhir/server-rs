//! octofhir-jsc - JavaScriptCore runtime for OctoFHIR automation
//!
//! This crate provides a safe Rust wrapper around Apple's JavaScriptCore engine
//! for executing JavaScript automations in OctoFHIR.
//!
//! # Features
//!
//! - **ES2020+ Support**: Full modern JavaScript syntax support (Safari-tested)
//! - **JIT Compilation**: Fast execution with JSC's multi-tier JIT compiler
//! - **Thread-safe Pool**: Multiple JSC contexts for concurrent execution
//! - **Native APIs**: Rust implementations of `fetch`, `fhir.*`, `console.*`
//! - **GC Protection**: Automatic garbage collection management
//!
//! # Example
//!
//! ```no_run
//! use octofhir_jsc::{JscRuntimePool, JscConfig};
//!
//! let config = JscConfig {
//!     pool_size: 4,
//!     timeout_ms: 5000,
//!     enable_console: true,
//! };
//!
//! let pool = JscRuntimePool::new(config).unwrap();
//!
//! // Simple evaluation
//! let result = pool.eval("2 + 2").unwrap();
//! assert_eq!(result.to_number().unwrap(), 4.0);
//!
//! // With context
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! struct Event {
//!     resource_type: String,
//!     id: String,
//! }
//!
//! let event = Event {
//!     resource_type: "Patient".to_string(),
//!     id: "123".to_string(),
//! };
//!
//! let result = pool.eval_with_context(
//!     "event.resource_type + '/' + event.id",
//!     "event",
//!     &event
//! ).unwrap();
//! assert_eq!(result.to_string().unwrap(), "Patient/123");
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    JscRuntimePool                            │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │
//! │  │ JscRuntime  │ │ JscRuntime  │ │ JscRuntime  │  ...      │
//! │  │  (Mutex)    │ │  (Mutex)    │ │  (Mutex)    │           │
//! │  └─────────────┘ └─────────────┘ └─────────────┘           │
//! │         │               │               │                   │
//! │         └───────────────┼───────────────┘                   │
//! │                         ↓                                   │
//! │              Round-robin selection                          │
//! └─────────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    JscContext                                │
//! │  - Script evaluation                                         │
//! │  - Object creation                                           │
//! │  - Native function registration                              │
//! └─────────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    JscValue                                  │
//! │  - GC-protected JavaScript values                            │
//! │  - Type conversion (to Rust types)                           │
//! │  - JSON serialization/deserialization                        │
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod apis;
pub mod bindings;
pub mod context;
pub mod error;
pub mod runtime;
pub mod transpiler;
pub mod value;

// Re-exports for convenience
pub use apis::fhir::{clear_fhir_client, set_fhir_client, FhirClient};
pub use apis::register_all_apis;
pub use context::JscContext;
pub use error::{JscError, JscResult};
pub use runtime::{AutomationContext, AutomationEvent, JscConfig, JscRuntime, JscRuntimePool};
pub use transpiler::{
    is_typescript, transpile_typescript, transpile_typescript_with_options, TranspileError,
    TranspileOptions, TranspileResult,
};
pub use value::JscValue;

/// Prelude module for common imports
pub mod prelude {
    pub use crate::apis::fhir::{clear_fhir_client, set_fhir_client, FhirClient};
    pub use crate::apis::register_all_apis;
    pub use crate::context::JscContext;
    pub use crate::error::{JscError, JscResult};
    pub use crate::runtime::{AutomationContext, AutomationEvent, JscConfig, JscRuntime, JscRuntimePool};
    pub use crate::transpiler::{
        is_typescript, transpile_typescript, transpile_typescript_with_options, TranspileError,
        TranspileOptions, TranspileResult,
    };
    pub use crate::value::JscValue;
}
