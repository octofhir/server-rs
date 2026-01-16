//! Automation storage layer.

use async_trait::async_trait;
use sqlx_core::query::query;
use sqlx_core::row::Row;
use sqlx_postgres::PgPool;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

use super::types::{
    Automation, AutomationExecution, AutomationExecutionStats, AutomationExecutionStatus,
    AutomationStatus, AutomationTrigger, AutomationTriggerType, CreateAutomation,
    CreateAutomationTrigger, UpdateAutomation,
};

/// Automation storage trait
#[async_trait]
pub trait AutomationStorage: Send + Sync {
    /// Create a new automation
    async fn create_automation(
        &self,
        create: CreateAutomation,
    ) -> Result<Automation, AutomationStorageError>;

    /// Get an automation by ID
    async fn get_automation(&self, id: Uuid) -> Result<Option<Automation>, AutomationStorageError>;

    /// List all automations
    async fn list_automations(&self) -> Result<Vec<Automation>, AutomationStorageError>;

    /// Update an automation
    async fn update_automation(
        &self,
        id: Uuid,
        update: UpdateAutomation,
    ) -> Result<Option<Automation>, AutomationStorageError>;

    /// Deploy an automation (compile TypeScript and set status to active)
    async fn deploy_automation(
        &self,
        id: Uuid,
        compiled_code: String,
    ) -> Result<Option<Automation>, AutomationStorageError>;

    /// Delete an automation
    async fn delete_automation(&self, id: Uuid) -> Result<bool, AutomationStorageError>;

    /// Get triggers for an automation
    async fn get_triggers(
        &self,
        automation_id: Uuid,
    ) -> Result<Vec<AutomationTrigger>, AutomationStorageError>;

    /// Create a trigger for an automation
    async fn create_trigger(
        &self,
        automation_id: Uuid,
        trigger: CreateAutomationTrigger,
    ) -> Result<AutomationTrigger, AutomationStorageError>;

    /// Delete a trigger
    async fn delete_trigger(&self, trigger_id: Uuid) -> Result<bool, AutomationStorageError>;

    /// Get active automations matching a resource event
    async fn get_matching_automations(
        &self,
        resource_type: &str,
        event_type: &str,
    ) -> Result<Vec<(Automation, AutomationTrigger)>, AutomationStorageError>;

    /// Log an automation execution
    async fn log_execution(
        &self,
        execution: AutomationExecution,
    ) -> Result<(), AutomationStorageError>;

    /// Get executions for an automation
    async fn get_executions(
        &self,
        automation_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AutomationExecution>, AutomationStorageError>;

    /// Get execution statistics for multiple automations in batch
    async fn get_execution_stats_batch(
        &self,
        automation_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, AutomationExecutionStats>, AutomationStorageError>;
}

/// Automation storage error
#[derive(Debug, thiserror::Error)]
pub enum AutomationStorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx_core::error::Error),

    #[error("Automation not found: {0}")]
    NotFound(Uuid),

    #[error("Invalid trigger configuration: {0}")]
    InvalidTrigger(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// PostgreSQL implementation of automation storage
pub struct PostgresAutomationStorage {
    pool: PgPool,
}

impl PostgresAutomationStorage {
    /// Create a new PostgreSQL automation storage
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn row_to_automation(row: &sqlx_postgres::PgRow) -> Result<Automation, AutomationStorageError> {
        let status_str: String = row.try_get("status")?;
        let status = AutomationStatus::from_str(&status_str).ok_or_else(|| {
            AutomationStorageError::InvalidData(format!("Invalid status: {}", status_str))
        })?;

        Ok(Automation {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            source_code: row.try_get("source_code")?,
            compiled_code: row.try_get("compiled_code")?,
            status,
            version: row.try_get("version")?,
            timeout_ms: row.try_get("timeout_ms")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    fn row_to_trigger(
        row: &sqlx_postgres::PgRow,
    ) -> Result<AutomationTrigger, AutomationStorageError> {
        let trigger_type_str: String = row.try_get("trigger_type")?;
        let trigger_type = AutomationTriggerType::from_str(&trigger_type_str).ok_or_else(|| {
            AutomationStorageError::InvalidData(format!(
                "Invalid trigger type: {}",
                trigger_type_str
            ))
        })?;

        Ok(AutomationTrigger {
            id: row.try_get("id")?,
            automation_id: row.try_get("automation_id")?,
            trigger_type,
            resource_type: row.try_get("resource_type")?,
            event_types: row.try_get("event_types")?,
            fhirpath_filter: row.try_get("fhirpath_filter")?,
            cron_expression: row.try_get("cron_expression")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

#[async_trait]
impl AutomationStorage for PostgresAutomationStorage {
    async fn create_automation(
        &self,
        create: CreateAutomation,
    ) -> Result<Automation, AutomationStorageError> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        let row = query(
            r#"
            INSERT INTO automation (id, name, description, source_code, compiled_code, status, version, timeout_ms, created_at, updated_at)
            VALUES ($1, $2, $3, $4, NULL, 'inactive', 1, $5, $6, $6)
            RETURNING id, name, description, source_code, compiled_code, status::TEXT, version, timeout_ms, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&create.name)
        .bind(&create.description)
        .bind(&create.source_code)
        .bind(create.timeout_ms)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        let automation = Self::row_to_automation(&row)?;

        // Create triggers
        for trigger in create.triggers {
            self.create_trigger(automation.id, trigger).await?;
        }

        Ok(automation)
    }

    async fn get_automation(&self, id: Uuid) -> Result<Option<Automation>, AutomationStorageError> {
        let row = query(
            r#"
            SELECT id, name, description, source_code, compiled_code, status::TEXT, version, timeout_ms, created_at, updated_at
            FROM automation
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Self::row_to_automation(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_automations(&self) -> Result<Vec<Automation>, AutomationStorageError> {
        let rows = query(
            r#"
            SELECT id, name, description, source_code, compiled_code, status::TEXT, version, timeout_ms, created_at, updated_at
            FROM automation
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_automation).collect()
    }

    async fn update_automation(
        &self,
        id: Uuid,
        update: UpdateAutomation,
    ) -> Result<Option<Automation>, AutomationStorageError> {
        let now = OffsetDateTime::now_utc();

        // Get current automation
        let current = match self.get_automation(id).await? {
            Some(automation) => automation,
            None => return Ok(None),
        };

        let name = update.name.unwrap_or(current.name);
        let description = update.description.or(current.description);
        let source_code_changed = update.source_code.is_some();
        let source_code = update.source_code.unwrap_or(current.source_code);
        let status = update.status.unwrap_or(current.status);
        let timeout_ms = update.timeout_ms.unwrap_or(current.timeout_ms);
        // Clear compiled_code if source_code changed (will be recompiled on deploy)
        let compiled_code: Option<String> = if source_code_changed {
            None
        } else {
            current.compiled_code
        };

        let row = query(
            r#"
            UPDATE automation
            SET name = $2, description = $3, source_code = $4, compiled_code = $5, status = $6::automation_status, timeout_ms = $7, version = version + 1, updated_at = $8
            WHERE id = $1
            RETURNING id, name, description, source_code, compiled_code, status::TEXT, version, timeout_ms, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&name)
        .bind(&description)
        .bind(&source_code)
        .bind(&compiled_code)
        .bind(status.as_str())
        .bind(timeout_ms)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Self::row_to_automation(&row)?)),
            None => Ok(None),
        }
    }

    async fn deploy_automation(
        &self,
        id: Uuid,
        compiled_code: String,
    ) -> Result<Option<Automation>, AutomationStorageError> {
        let now = OffsetDateTime::now_utc();

        let row = query(
            r#"
            UPDATE automation
            SET compiled_code = $2, status = 'active'::automation_status, updated_at = $3
            WHERE id = $1
            RETURNING id, name, description, source_code, compiled_code, status::TEXT, version, timeout_ms, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&compiled_code)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(Self::row_to_automation(&row)?)),
            None => Ok(None),
        }
    }

    async fn delete_automation(&self, id: Uuid) -> Result<bool, AutomationStorageError> {
        let result = query("DELETE FROM automation WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn get_triggers(
        &self,
        automation_id: Uuid,
    ) -> Result<Vec<AutomationTrigger>, AutomationStorageError> {
        let rows = query(
            r#"
            SELECT id, automation_id, trigger_type::TEXT, resource_type, event_types, fhirpath_filter, cron_expression, created_at
            FROM automation_trigger
            WHERE automation_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(automation_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_trigger).collect()
    }

    async fn create_trigger(
        &self,
        automation_id: Uuid,
        trigger: CreateAutomationTrigger,
    ) -> Result<AutomationTrigger, AutomationStorageError> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        // Validate trigger configuration
        match trigger.trigger_type {
            AutomationTriggerType::ResourceEvent => {
                if trigger.resource_type.is_none() {
                    return Err(AutomationStorageError::InvalidTrigger(
                        "resource_type is required for resource_event triggers".to_string(),
                    ));
                }
            }
            AutomationTriggerType::Cron => {
                if trigger.cron_expression.is_none() {
                    return Err(AutomationStorageError::InvalidTrigger(
                        "cron_expression is required for cron triggers".to_string(),
                    ));
                }
            }
            AutomationTriggerType::Manual => {
                // No additional validation needed
            }
        }

        let row = query(
            r#"
            INSERT INTO automation_trigger (id, automation_id, trigger_type, resource_type, event_types, fhirpath_filter, cron_expression, created_at)
            VALUES ($1, $2, $3::automation_trigger_type, $4, $5, $6, $7, $8)
            RETURNING id, automation_id, trigger_type::TEXT, resource_type, event_types, fhirpath_filter, cron_expression, created_at
            "#,
        )
        .bind(id)
        .bind(automation_id)
        .bind(trigger.trigger_type.as_str())
        .bind(&trigger.resource_type)
        .bind(&trigger.event_types)
        .bind(&trigger.fhirpath_filter)
        .bind(&trigger.cron_expression)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Self::row_to_trigger(&row)
    }

    async fn delete_trigger(&self, trigger_id: Uuid) -> Result<bool, AutomationStorageError> {
        let result = query("DELETE FROM automation_trigger WHERE id = $1")
            .bind(trigger_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn get_matching_automations(
        &self,
        resource_type: &str,
        event_type: &str,
    ) -> Result<Vec<(Automation, AutomationTrigger)>, AutomationStorageError> {
        // Query for active automations with matching triggers
        let rows = query(
            r#"
            SELECT
                a.id as automation_id, a.name, a.description, a.source_code, a.compiled_code, a.status::TEXT, a.version, a.timeout_ms, a.created_at as automation_created_at, a.updated_at,
                t.id as trigger_id, t.automation_id as trigger_automation_id, t.trigger_type::TEXT, t.resource_type, t.event_types, t.fhirpath_filter, t.cron_expression, t.created_at as trigger_created_at
            FROM automation a
            JOIN automation_trigger t ON a.id = t.automation_id
            WHERE a.status = 'active'
              AND t.trigger_type = 'resource_event'
              AND t.resource_type = $1
              AND ($2 = ANY(t.event_types) OR t.event_types IS NULL)
            ORDER BY a.created_at
            "#,
        )
        .bind(resource_type)
        .bind(event_type)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let status_str: String = row.try_get("status")?;
            let status = AutomationStatus::from_str(&status_str).ok_or_else(|| {
                AutomationStorageError::InvalidData(format!("Invalid status: {}", status_str))
            })?;

            let trigger_type_str: String = row.try_get("trigger_type")?;
            let trigger_type =
                AutomationTriggerType::from_str(&trigger_type_str).ok_or_else(|| {
                    AutomationStorageError::InvalidData(format!(
                        "Invalid trigger type: {}",
                        trigger_type_str
                    ))
                })?;

            let automation = Automation {
                id: row.try_get("automation_id")?,
                name: row.try_get("name")?,
                description: row.try_get("description")?,
                source_code: row.try_get("source_code")?,
                compiled_code: row.try_get("compiled_code")?,
                status,
                version: row.try_get("version")?,
                timeout_ms: row.try_get("timeout_ms")?,
                created_at: row.try_get("automation_created_at")?,
                updated_at: row.try_get("updated_at")?,
            };
            let trigger = AutomationTrigger {
                id: row.try_get("trigger_id")?,
                automation_id: row.try_get("trigger_automation_id")?,
                trigger_type,
                resource_type: row.try_get("resource_type")?,
                event_types: row.try_get("event_types")?,
                fhirpath_filter: row.try_get("fhirpath_filter")?,
                cron_expression: row.try_get("cron_expression")?,
                created_at: row.try_get("trigger_created_at")?,
            };
            results.push((automation, trigger));
        }

        Ok(results)
    }

    async fn log_execution(
        &self,
        execution: AutomationExecution,
    ) -> Result<(), AutomationStorageError> {
        // Serialize logs to JSON
        let logs_json = execution
            .logs
            .as_ref()
            .map(|logs| serde_json::to_value(logs).ok())
            .flatten();

        query(
            r#"
            INSERT INTO automation_execution (id, automation_id, trigger_id, status, input, output, error, started_at, completed_at, duration_ms, logs)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE SET
                status = EXCLUDED.status,
                output = EXCLUDED.output,
                error = EXCLUDED.error,
                completed_at = EXCLUDED.completed_at,
                duration_ms = EXCLUDED.duration_ms,
                logs = EXCLUDED.logs
            "#,
        )
        .bind(execution.id)
        .bind(execution.automation_id)
        .bind(execution.trigger_id)
        .bind(execution.status.as_str())
        .bind(&execution.input)
        .bind(&execution.output)
        .bind(&execution.error)
        .bind(execution.started_at)
        .bind(execution.completed_at)
        .bind(execution.duration_ms)
        .bind(&logs_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_executions(
        &self,
        automation_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AutomationExecution>, AutomationStorageError> {
        let rows = query(
            r#"
            SELECT id, automation_id, trigger_id, status, input, output, error, started_at, completed_at, duration_ms, logs
            FROM automation_execution
            WHERE automation_id = $1
            ORDER BY started_at DESC
            LIMIT $2
            "#,
        )
        .bind(automation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut executions = Vec::new();
        for row in rows {
            let status_str: String = row.try_get("status")?;
            let status = AutomationExecutionStatus::from_str(&status_str)
                .unwrap_or(AutomationExecutionStatus::Failed);

            // Parse logs from JSON
            let logs_json: Option<serde_json::Value> = row.try_get("logs")?;
            let logs = logs_json.and_then(|v| serde_json::from_value(v).ok());

            executions.push(AutomationExecution {
                id: row.try_get("id")?,
                automation_id: row.try_get("automation_id")?,
                trigger_id: row.try_get("trigger_id")?,
                status,
                input: row.try_get("input")?,
                output: row.try_get("output")?,
                error: row.try_get("error")?,
                started_at: row.try_get("started_at")?,
                completed_at: row.try_get("completed_at")?,
                duration_ms: row.try_get("duration_ms")?,
                logs,
            });
        }

        Ok(executions)
    }

    async fn get_execution_stats_batch(
        &self,
        automation_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, AutomationExecutionStats>, AutomationStorageError> {
        if automation_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Query to get stats for all automation IDs at once
        // Uses CTEs to efficiently compute:
        // - Last execution status and timestamp
        // - Last error message
        // - Counts for last 24 hours
        let rows = query(
            r#"
            WITH last_executions AS (
                SELECT DISTINCT ON (automation_id)
                    automation_id,
                    status,
                    error,
                    started_at
                FROM automation_execution
                WHERE automation_id = ANY($1)
                ORDER BY automation_id, started_at DESC
            ),
            counts_24h AS (
                SELECT
                    automation_id,
                    COUNT(CASE WHEN status = 'completed' THEN 1 END) as success_count,
                    COUNT(CASE WHEN status = 'failed' THEN 1 END) as failure_count
                FROM automation_execution
                WHERE automation_id = ANY($1)
                  AND started_at > NOW() - INTERVAL '24 hours'
                GROUP BY automation_id
            )
            SELECT
                le.automation_id,
                le.status as last_status,
                le.error as last_error,
                le.started_at as last_execution_at,
                COALESCE(c.success_count, 0)::INT as success_count_24h,
                COALESCE(c.failure_count, 0)::INT as failure_count_24h
            FROM last_executions le
            LEFT JOIN counts_24h c ON le.automation_id = c.automation_id
            "#,
        )
        .bind(automation_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut stats_map = HashMap::new();
        for row in rows {
            let automation_id: Uuid = row.try_get("automation_id")?;
            let last_status: Option<String> = row.try_get("last_status")?;
            let last_error: Option<String> = row.try_get("last_error")?;
            let last_execution_at: Option<OffsetDateTime> = row.try_get("last_execution_at")?;
            let success_count_24h: i32 = row.try_get("success_count_24h")?;
            let failure_count_24h: i32 = row.try_get("failure_count_24h")?;

            stats_map.insert(
                automation_id,
                AutomationExecutionStats {
                    last_execution_status: last_status,
                    last_execution_at: last_execution_at.map(|t| t.to_string()),
                    last_error,
                    failure_count_24h,
                    success_count_24h,
                },
            );
        }

        Ok(stats_map)
    }
}
