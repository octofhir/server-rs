//! Automation execution engine.
//!
//! This module provides the `AutomationExecutor` which manages JSC runtime pools
//! and executes automations with proper context injection.

use octofhir_storage::FhirStorage;
use otter_runtime::{JscConfig, JscRuntimePool};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::runtime::Handle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::fhir_api::{clear_fhir_client, register_fhir_api, set_fhir_client};
use super::fhir_client::StorageFhirClient;
use super::storage::AutomationStorage;
use super::types::{
    Automation, AutomationEvent, AutomationExecution, AutomationExecutionStatus, AutomationTrigger,
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
    /// Execution duration
    pub duration: Duration,
}

/// Automation executor manages JSC runtime pools and executes automations
pub struct AutomationExecutor {
    #[allow(dead_code)]
    runtime_pool: JscRuntimePool,
    storage: Arc<dyn FhirStorage>,
    automation_storage: Arc<dyn AutomationStorage>,
    #[allow(dead_code)]
    config: AutomationExecutorConfig,
}

impl AutomationExecutor {
    /// Create a new automation executor
    pub fn new(
        storage: Arc<dyn FhirStorage>,
        automation_storage: Arc<dyn AutomationStorage>,
        config: AutomationExecutorConfig,
    ) -> Result<Self, String> {
        let jsc_config = JscConfig {
            pool_size: config.pool_size,
            timeout_ms: config.default_timeout_ms,
            enable_console: config.enable_console,
        };

        let runtime_pool = JscRuntimePool::new(jsc_config)
            .map_err(|e| format!("Failed to create JSC pool: {}", e))?;

        info!(
            pool_size = config.pool_size,
            timeout_ms = config.default_timeout_ms,
            "Automation executor initialized"
        );

        Ok(Self {
            runtime_pool,
            storage,
            automation_storage,
            config,
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
        };

        if let Err(e) = self.automation_storage.log_execution(execution).await {
            warn!(error = %e, "Failed to log execution start");
        }

        // Create FHIR client for this execution
        let handle = Handle::current();
        let fhir_client = Arc::new(StorageFhirClient::new(self.storage.clone(), handle));

        // Execute in a blocking context since JSC is not async
        // We need to convert the result to a type that is Send-safe
        // Use compiled_code if available (pre-transpiled at deploy time), otherwise fall back to source_code
        let js_code = automation
            .compiled_code
            .clone()
            .unwrap_or_else(|| automation.source_code.clone());
        let automation_event = event.clone();

        let result: Result<Option<serde_json::Value>, String> =
            tokio::task::spawn_blocking(move || {
                // Set FHIR client for this thread
                set_fhir_client(fhir_client);

                // Execute the automation
                let pool = match JscRuntimePool::new(JscConfig {
                    pool_size: 1,
                    timeout_ms: 5000,
                    enable_console: true,
                }) {
                    Ok(p) => p,
                    Err(e) => {
                        clear_fhir_client();
                        return Err(format!("Failed to create JSC runtime: {}", e));
                    }
                };

                if let Err(e) = pool.register_apis(register_fhir_api) {
                    clear_fhir_client();
                    return Err(format!("Failed to register FHIR API: {}", e));
                }

                // Wrap the automation code in a function to allow return statements
                // The function is immediately invoked with the event context
                let wrapped_code = format!("(function() {{\n{}\n}})()", js_code);
                let result = pool.eval_with_context(&wrapped_code, "event", &automation_event);

                // Clean up
                clear_fhir_client();

                // Convert JscValue to JSON (JscValue is not Send-safe)
                match result {
                    Ok(value) => {
                        let json_result = value
                            .to_json()
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                        Ok(json_result)
                    }
                    Err(e) => Err(e.to_string()),
                }
            })
            .await
            .unwrap_or_else(|e| Err(format!("Task panicked: {}", e)));

        let duration = start.elapsed();
        let completed_at = OffsetDateTime::now_utc();

        // Process result
        let (success, output, error) = match result {
            Ok(json_output) => (true, json_output, None),
            Err(e) => (false, None, Some(e)),
        };

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
                "Automation execution completed successfully"
            );
        } else {
            error!(
                automation_id = %automation.id,
                automation_name = %automation.name,
                execution_id = %execution_id,
                error = ?error,
                duration_ms = duration.as_millis() as u64,
                "Automation execution failed"
            );
        }

        ExecutionResult {
            execution_id,
            success,
            output,
            error,
            duration,
        }
    }

    /// Get the JSC runtime pool size
    pub fn pool_size(&self) -> usize {
        self.runtime_pool.pool_size()
    }

    /// Force garbage collection on all runtime instances
    pub fn gc(&self) {
        self.runtime_pool.gc_all();
    }
}
