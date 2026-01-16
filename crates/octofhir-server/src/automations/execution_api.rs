//! Execution Logging API for automation execution.
//!
//! Provides `execution.log`, `execution.info`, `execution.debug`, `execution.warn`, `execution.error`
//! for structured logging from automations. Logs are captured and stored in the execution record.
//!
//! # Implementation Note
//!
//! The execution logging API is injected directly into the JavaScript wrapper code in `executor.rs`
//! rather than using a separate otter_runtime extension. This approach was chosen because:
//!
//! 1. The extension state model in otter_runtime doesn't easily allow retrieving logs after
//!    `engine_handle.eval()` completes (state is per-runtime, not per-execution).
//!
//! 2. By injecting the logging code directly into the wrapped JavaScript, we can return logs
//!    alongside the execution result using a structured response object.
//!
//! # Usage in automations
//!
//! ```javascript
//! export default async function(ctx) {
//!     execution.log("Processing patient", { patientId: ctx.event.resource.id });
//!     execution.info("Validation passed");
//!     execution.debug("Raw data", ctx.event.resource);
//!     execution.warn("Missing optional field", { field: "birthDate" });
//!     execution.error("Failed to create task", { error: e.message });
//! }
//! ```
//!
//! # Log Entry Structure
//!
//! Each log entry contains:
//! - `level`: "log", "info", "debug", "warn", or "error"
//! - `message`: The log message (string)
//! - `data`: Optional structured data (any JSON value)
//! - `timestamp`: ISO 8601 timestamp of when the log was created
//!
//! # TypeScript Types
//!
//! TypeScript definitions for the execution API are provided in the Monaco editor
//! (`AutomationScriptEditor.tsx`) for IDE support:
//!
//! ```typescript
//! declare const execution: {
//!   log(message: string, data?: unknown): void;
//!   info(message: string, data?: unknown): void;
//!   debug(message: string, data?: unknown): void;
//!   warn(message: string, data?: unknown): void;
//!   error(message: string, data?: unknown): void;
//! };
//! ```

// This module serves as documentation for the execution logging API.
// The actual implementation is in executor.rs (JavaScript wrapper injection).
