//! Automation execution engine.
//!
//! This module provides the `AutomationExecutor` which manages the otter Engine
//! and executes automations with proper context injection.

use octofhir_storage::FhirStorage;
// use otter_runtime::{Engine, EngineBuilder, EngineHandle, set_net_permission_checker};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::runtime::Handle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::fhir_api::{FhirContext, fhir_extension};
use super::storage::AutomationStorage;
use super::types::{
    Automation, AutomationEvent, AutomationExecution, AutomationExecutionStatus,
    AutomationLogEntry, AutomationTrigger,
};

/// Configuration for the automation executor
#[derive(Debug, Clone)]
pub struct AutomationExecutorConfig {
    /// Number of JSC runtime instances in the pool
    pub pool_size: usize,
    /// Default timeout for automation execution in milliseconds
    pub default_timeout_ms: u64,
    /// Enable console output from automations
    pub enable_console: bool,
}

impl Default for AutomationExecutorConfig {
    fn default() -> Self {
        Self {
            pool_size: num_cpus::get().max(1),
            default_timeout_ms: 5000,
            enable_console: true,
        }
    }
}

/// Automation execution result
#[derive(Debug)]
pub struct ExecutionResult {
    /// Execution ID
    pub execution_id: Uuid,
    /// Whether execution succeeded
    pub success: bool,
    /// Output value (if any)
    pub output: Option<serde_json::Value>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Structured logs from execution.log() API
    pub logs: Vec<AutomationLogEntry>,
    /// Execution duration
    pub duration: Duration,
}

/// Automation executor manages the otter Engine and executes automations
pub struct AutomationExecutor {
    /*
    engine_handle: EngineHandle,
    automation_storage: Arc<dyn AutomationStorage>,
    #[allow(dead_code)]
    config: AutomationExecutorConfig,
    // Keep Engine alive
    #[allow(dead_code)]
    engine: Engine,
    */
    automation_storage: Arc<dyn AutomationStorage>,
    config: AutomationExecutorConfig,
}

impl AutomationExecutor {
    /// Create a new automation executor
    pub fn new(
        _storage: Arc<dyn FhirStorage>,
        automation_storage: Arc<dyn AutomationStorage>,
        config: AutomationExecutorConfig,
    ) -> Result<Self, String> {
        /*
        let handle = Handle::current();

        // Enable network access for built-in fetch() in automations
        set_net_permission_checker(Box::new(|_host| true));

        // Create FHIR extension with init that sets up the context
        // ExtensionState::put wraps in Arc automatically, so pass FhirContext directly
        let fhir_ext = fhir_extension().with_init({
            let storage = storage.clone();
            let handle = handle.clone();
            move |state| {
                state.put(FhirContext {
                    storage: storage.clone(),
                    handle: handle.clone(),
                });
            }
        });

        // Build engine with FHIR extension (uses built-in fetch() for HTTP)
        let engine = EngineBuilder::default()
            .pool_size(config.pool_size)
            .extension(fhir_ext)
            .build()
            .map_err(|e| format!("Failed to create otter Engine: {}", e))?;

        let engine_handle = engine.handle();

        info!(
            pool_size = config.pool_size,
            timeout_ms = config.default_timeout_ms,
            "Automation executor initialized with otter Engine"
        );
        */

        Ok(Self {
            // engine_handle,
            automation_storage,
            config,
            // engine,
        })
    }

    /// Execute an automation with the given context
    pub async fn execute(
        &self,
        automation: &Automation,
        trigger: Option<&AutomationTrigger>,
        event: AutomationEvent,
    ) -> ExecutionResult {
        let execution_id = Uuid::new_v4();
        let start = Instant::now();
        let started_at = OffsetDateTime::now_utc();

        debug!(
            automation_id = %automation.id,
            automation_name = %automation.name,
            execution_id = %execution_id,
            event_type = %event.event_type,
            "Starting automation execution"
        );

        // Log execution start
        let execution = AutomationExecution {
            id: execution_id,
            automation_id: automation.id,
            trigger_id: trigger.map(|t| t.id),
            status: AutomationExecutionStatus::Running,
            input: Some(json!({
                "event": event,
            })),
            output: None,
            error: None,
            started_at,
            completed_at: None,
            duration_ms: None,
            logs: None,
        };

        if let Err(e) = self.automation_storage.log_execution(execution).await {
            warn!(error = %e, "Failed to log execution start");
        }

        // Use compiled_code if available (pre-transpiled at deploy time), otherwise fall back to source_code
        let js_code = automation
            .compiled_code
            .clone()
            .unwrap_or_else(|| automation.source_code.clone());

        // Build the execution context
        let ctx_json = serde_json::to_string(&json!({ "event": event }))
            .unwrap_or_else(|_| r#"{"event":{}}"#.to_string());

        // Transform ES module export default syntax to executable code
        // Replace "export default" with variable assignment
        let transformed_code = js_code
            .replace("export default async function", "var __defaultExport = async function")
            .replace("export default async (", "var __defaultExport = async (")
            .replace("export default function", "var __defaultExport = function");

        // Wrap in async IIFE that:
        // 1. Sets up the execution.log() API
        // 2. Calls the default export with context
        // 3. Returns result and captured logs
        let wrapped_code = format!(
            r#"
(async function() {{
    // Set up execution logging API
    const __executionLogs = [];
    const __logWithLevel = (level, message, data) => {{
        __executionLogs.push({{
            level,
            message: typeof message === 'string' ? message : JSON.stringify(message),
            data: data !== undefined ? data : null,
            timestamp: new Date().toISOString()
        }});
    }};
    globalThis.execution = {{
        log: (message, data) => __logWithLevel('log', message, data),
        info: (message, data) => __logWithLevel('info', message, data),
        debug: (message, data) => __logWithLevel('debug', message, data),
        warn: (message, data) => __logWithLevel('warn', message, data),
        error: (message, data) => __logWithLevel('error', message, data),
    }};

    {transformed_code}

    if (typeof __defaultExport !== 'function') {{
        throw new Error('Automation must export default async function(ctx)');
    }}

    try {{
        const __result = await __defaultExport({ctx_json});
        return {{ __success: true, __result, __logs: __executionLogs }};
    }} catch (__err) {{
        return {{ __success: false, __error: __err.message || String(__err), __logs: __executionLogs }};
    }}
}})()
"#
        );

        /*
        // Execute using async EngineHandle
        let result = self.engine_handle.eval(&wrapped_code).await;

        let duration = start.elapsed();
        let completed_at = OffsetDateTime::now_utc();

        // Process result - extract success, output, error, and logs from wrapped response
        let (success, output, error, logs) = match result {
            Ok(json_output) => {
                // Parse the wrapped response: { __success, __result/__error, __logs }
                let is_success = json_output
                    .get("__success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let output = json_output
                    .get("__result")
                    .cloned()
                    .filter(|v| !v.is_null());

                let error = json_output
                    .get("__error")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                // Parse logs from the response
                let logs: Vec<AutomationLogEntry> = json_output
                    .get("__logs")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                (is_success, output, error, logs)
            }
            Err(e) => (false, None, Some(e.to_string()), Vec::new()),
        };
        */
        let (success, output, error, logs) = (false, None, Some("Automation disabled".to_string()), Vec::new());
        let duration = start.elapsed();
        let completed_at = OffsetDateTime::now_utc();

        // Log execution completion
        let status = if success {
            AutomationExecutionStatus::Completed
        } else {
            AutomationExecutionStatus::Failed
        };

        let execution = AutomationExecution {
            id: execution_id,
            automation_id: automation.id,
            trigger_id: trigger.map(|t| t.id),
            status,
            input: Some(json!({
                "event": event,
            })),
            output: output.clone(),
            error: error.clone(),
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some(duration.as_millis() as i32),
            logs: if logs.is_empty() { None } else { Some(logs.clone()) },
        };

        if let Err(e) = self.automation_storage.log_execution(execution).await {
            warn!(error = %e, "Failed to log execution completion");
        }

        if success {
            info!(
                automation_id = %automation.id,
                automation_name = %automation.name,
                execution_id = %execution_id,
                duration_ms = duration.as_millis() as u64,
                log_count = logs.len(),
                "Automation execution completed successfully"
            );
        } else {
            error!(
                automation_id = %automation.id,
                automation_name = %automation.name,
                execution_id = %execution_id,
                error = ?error,
                duration_ms = duration.as_millis() as u64,
                log_count = logs.len(),
                "Automation execution failed"
            );
        }

        ExecutionResult {
            execution_id,
            success,
            output,
            error,
            logs,
            duration,
        }
    }

    /// Get the Engine pool size
    pub fn pool_size(&self) -> usize {
        // self.engine.pool_size()
        0
    }

    /// Get engine statistics
    /*
    pub fn stats(&self) -> otter_runtime::EngineStatsSnapshot {
        self.engine.stats().snapshot()
    }
    */
}
