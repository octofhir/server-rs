//! Cron scheduler for automation system.
//!
//! This module provides a background scheduler that executes automations based on cron expressions.
//! Automations with `cron` triggers will be executed according to their schedule.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::types::AutomationEvent;
use croner::Cron;
use time::OffsetDateTime;
use tokio::sync::watch;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use super::executor::AutomationExecutor;
use super::storage::AutomationStorage;
use super::types::{Automation, AutomationStatus, AutomationTrigger, AutomationTriggerType};

/// Configuration for the cron scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// How often to check for automations that need to run (in seconds).
    /// Default: 60 seconds (check every minute)
    pub check_interval_secs: u64,

    /// Whether to run missed executions on startup.
    /// Default: false (skip missed executions)
    pub run_missed_on_startup: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 60,
            run_missed_on_startup: false,
        }
    }
}

/// Cron scheduler that runs automations according to their cron schedules.
pub struct CronScheduler {
    automation_storage: Arc<dyn AutomationStorage>,
    executor: Arc<AutomationExecutor>,
    config: SchedulerConfig,
    /// Tracks the last run time for each automation+trigger combination
    last_runs: HashMap<(uuid::Uuid, uuid::Uuid), OffsetDateTime>,
}

impl CronScheduler {
    /// Create a new cron scheduler.
    pub fn new(
        automation_storage: Arc<dyn AutomationStorage>,
        executor: Arc<AutomationExecutor>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            automation_storage,
            executor,
            config,
            last_runs: HashMap::new(),
        }
    }

    /// Start the scheduler in a background task.
    ///
    /// Returns a shutdown sender that can be used to stop the scheduler.
    pub fn start(mut self) -> watch::Sender<bool> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        tokio::spawn(async move {
            info!(
                check_interval_secs = self.config.check_interval_secs,
                "Cron scheduler started"
            );

            let mut ticker = interval(Duration::from_secs(self.config.check_interval_secs));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if let Err(e) = self.check_and_run().await {
                            error!(error = %e, "Error in cron scheduler tick");
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Cron scheduler shutting down");
                            break;
                        }
                    }
                }
            }
        });

        shutdown_tx
    }

    /// Check all cron-triggered automations and run those that are due.
    async fn check_and_run(&mut self) -> Result<(), String> {
        let now = OffsetDateTime::now_utc();

        // Get all active automations with cron triggers
        let automations_with_triggers = self.get_cron_automations().await?;

        for (automation, trigger) in automations_with_triggers {
            let cron_expr = match &trigger.cron_expression {
                Some(expr) => expr,
                None => {
                    warn!(
                        automation_id = %automation.id,
                        trigger_id = %trigger.id,
                        "Cron trigger missing cron_expression"
                    );
                    continue;
                }
            };

            // Parse cron expression
            let cron = match Cron::new(cron_expr).parse() {
                Ok(c) => c,
                Err(e) => {
                    error!(
                        automation_id = %automation.id,
                        cron_expression = %cron_expr,
                        error = %e,
                        "Invalid cron expression"
                    );
                    continue;
                }
            };

            // Check if it's time to run
            let key = (automation.id, trigger.id);
            let last_run = self.last_runs.get(&key).copied();

            if self.should_run(&cron, now, last_run) {
                debug!(
                    automation_id = %automation.id,
                    automation_name = %automation.name,
                    cron_expression = %cron_expr,
                    "Executing scheduled automation"
                );

                // Update last run time before executing
                self.last_runs.insert(key, now);

                // Execute the automation
                self.execute_automation(&automation, &trigger).await;
            }
        }

        Ok(())
    }

    /// Get all active automations with cron triggers.
    async fn get_cron_automations(&self) -> Result<Vec<(Automation, AutomationTrigger)>, String> {
        // Get all active automations
        let automations = self
            .automation_storage
            .list_automations()
            .await
            .map_err(|e| e.to_string())?;

        let mut result = Vec::new();

        for automation in automations {
            // Skip inactive automations
            if automation.status != AutomationStatus::Active {
                continue;
            }

            // Get triggers for this automation
            let triggers = self
                .automation_storage
                .get_triggers(automation.id)
                .await
                .map_err(|e| e.to_string())?;

            // Find cron triggers
            for trigger in triggers {
                if trigger.trigger_type == AutomationTriggerType::Cron {
                    result.push((automation.clone(), trigger));
                }
            }
        }

        Ok(result)
    }

    /// Check if an automation should run based on its cron schedule.
    fn should_run(
        &self,
        cron: &Cron,
        now: OffsetDateTime,
        last_run: Option<OffsetDateTime>,
    ) -> bool {
        // Convert time to chrono for croner compatibility
        let now_chrono = chrono::DateTime::from_timestamp(now.unix_timestamp(), 0)
            .unwrap_or_else(chrono::Utc::now);

        // Get the previous scheduled time (looking backward)
        // We need to step back one interval and find the next occurrence from there
        let check_window = chrono::Duration::seconds(self.config.check_interval_secs as i64 * 2);
        let past_time = now_chrono - check_window;

        let prev = match cron.find_next_occurrence(&past_time, false) {
            Ok(prev) => prev,
            Err(_) => return false,
        };

        // Check if we're within the check window of a scheduled time
        let window_secs = self.config.check_interval_secs as i64;
        let now_ts = now_chrono.timestamp();
        let prev_ts = prev.timestamp();

        // Skip if prev is in the future (past our now)
        if prev_ts > now_ts {
            return false;
        }

        // If we haven't run this trigger yet, check if prev is within our window
        if last_run.is_none() {
            // Only run if prev is very recent (within our check interval)
            return (now_ts - prev_ts).abs() < window_secs;
        }

        let last_run_ts = last_run.unwrap().unix_timestamp();

        // Check if prev is after our last run and within our check window
        prev_ts > last_run_ts && (now_ts - prev_ts).abs() < window_secs
    }

    /// Execute an automation for a cron trigger.
    async fn execute_automation(&self, automation: &Automation, trigger: &AutomationTrigger) {
        let executor = self.executor.clone();
        let automation = automation.clone();
        let trigger = trigger.clone();

        // Create event for cron execution
        let event = AutomationEvent {
            event_type: "cron".to_string(),
            resource: serde_json::json!({
                "resourceType": "Parameters",
                "parameter": [{
                    "name": "trigger",
                    "valueString": "cron"
                }, {
                    "name": "cronExpression",
                    "valueString": trigger.cron_expression.as_deref().unwrap_or("")
                }, {
                    "name": "timestamp",
                    "valueDateTime": OffsetDateTime::now_utc().to_string()
                }]
            }),
            previous: None,
            timestamp: OffsetDateTime::now_utc().to_string(),
        };

        // Spawn execution as a background task
        tokio::spawn(async move {
            let result = executor
                .execute(&automation, Some(&trigger), event)
                .await;

            if result.success {
                info!(
                    automation_id = %automation.id,
                    automation_name = %automation.name,
                    execution_id = %result.execution_id,
                    duration_ms = result.duration.as_millis() as u64,
                    "Scheduled automation execution completed"
                );
            } else {
                error!(
                    automation_id = %automation.id,
                    automation_name = %automation.name,
                    execution_id = %result.execution_id,
                    error = ?result.error,
                    "Scheduled automation execution failed"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_parsing() {
        // Test various cron expressions
        let expressions = [
            "* * * * *",      // Every minute
            "0 * * * *",      // Every hour
            "0 0 * * *",      // Every day at midnight
            "0 0 * * 0",      // Every Sunday at midnight
            "*/5 * * * *",    // Every 5 minutes
            "0 9-17 * * 1-5", // 9am-5pm on weekdays
        ];

        for expr in expressions {
            let result = Cron::new(expr).parse();
            assert!(result.is_ok(), "Failed to parse: {}", expr);
        }
    }

    #[test]
    fn test_invalid_cron() {
        let invalid = [
            "",
            "invalid",
            "* * *",      // Too few fields
            "60 * * * *", // Invalid minute
        ];

        for expr in invalid {
            let result = Cron::new(expr).parse();
            assert!(result.is_err(), "Should fail: {}", expr);
        }
    }
}
