//! Automation type definitions.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Automation status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationStatus {
    /// Automation is active and will be triggered
    Active,
    /// Automation is inactive and will not be triggered
    Inactive,
    /// Automation is in error state (e.g., compilation failed)
    Error,
}

impl Default for AutomationStatus {
    fn default() -> Self {
        Self::Inactive
    }
}

impl AutomationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutomationStatus::Active => "active",
            AutomationStatus::Inactive => "inactive",
            AutomationStatus::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(AutomationStatus::Active),
            "inactive" => Some(AutomationStatus::Inactive),
            "error" => Some(AutomationStatus::Error),
            _ => None,
        }
    }
}

/// Automation trigger type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerType {
    /// Triggered by resource events (create, update, delete)
    ResourceEvent,
    /// Triggered by cron schedule
    Cron,
    /// Triggered manually via API
    Manual,
}

impl AutomationTriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutomationTriggerType::ResourceEvent => "resource_event",
            AutomationTriggerType::Cron => "cron",
            AutomationTriggerType::Manual => "manual",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "resource_event" => Some(AutomationTriggerType::ResourceEvent),
            "cron" => Some(AutomationTriggerType::Cron),
            "manual" => Some(AutomationTriggerType::Manual),
            _ => None,
        }
    }
}

/// Automation execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationExecutionStatus {
    /// Automation is currently running
    Running,
    /// Automation completed successfully
    Completed,
    /// Automation failed with an error
    Failed,
}

impl AutomationExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutomationExecutionStatus::Running => "running",
            AutomationExecutionStatus::Completed => "completed",
            AutomationExecutionStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(AutomationExecutionStatus::Running),
            "completed" => Some(AutomationExecutionStatus::Completed),
            "failed" => Some(AutomationExecutionStatus::Failed),
            _ => None,
        }
    }
}

/// Event that triggered an automation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationEvent {
    /// Event type: "created", "updated", "deleted", "cron", "manual"
    #[serde(rename = "type")]
    pub event_type: String,

    /// The resource that triggered the event
    pub resource: serde_json::Value,

    /// Previous version of the resource (for updates)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<serde_json::Value>,

    /// Timestamp of the event
    pub timestamp: String,
}

/// An automation definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Source code (TypeScript or JavaScript)
    pub source_code: String,
    /// Compiled JavaScript (transpiled from TypeScript at deploy time)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiled_code: Option<String>,
    /// Automation status
    pub status: AutomationStatus,
    /// Version number (incremented on each update)
    pub version: i32,
    /// Maximum execution time in milliseconds
    pub timeout_ms: i32,
    /// When the automation was created
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// When the automation was last updated
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A trigger configuration for an automation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationTrigger {
    /// Unique identifier
    pub id: Uuid,
    /// Automation this trigger belongs to
    pub automation_id: Uuid,
    /// Type of trigger
    pub trigger_type: AutomationTriggerType,
    /// Resource type for resource_event triggers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    /// Event types for resource_event triggers (e.g., ["created", "updated"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_types: Option<Vec<String>>,
    /// Optional FHIRPath filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhirpath_filter: Option<String>,
    /// Cron expression for cron triggers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_expression: Option<String>,
    /// When the trigger was created
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// An automation execution log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationExecution {
    /// Unique identifier
    pub id: Uuid,
    /// Automation that was executed
    pub automation_id: Uuid,
    /// Trigger that initiated execution (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<Uuid>,
    /// Execution status
    pub status: AutomationExecutionStatus,
    /// Input data (event context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    /// Output data (return value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When execution started
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    /// When execution completed
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    /// Execution duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i32>,
}

/// Request to create a new automation
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAutomation {
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// JavaScript source code
    pub source_code: String,
    /// Maximum execution time in milliseconds (default: 5000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: i32,
    /// Triggers for this automation
    #[serde(default)]
    pub triggers: Vec<CreateAutomationTrigger>,
}

fn default_timeout() -> i32 {
    5000
}

/// Request to create an automation trigger
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAutomationTrigger {
    /// Type of trigger
    pub trigger_type: AutomationTriggerType,
    /// Resource type for resource_event triggers
    #[serde(default)]
    pub resource_type: Option<String>,
    /// Event types for resource_event triggers
    #[serde(default)]
    pub event_types: Option<Vec<String>>,
    /// Optional FHIRPath filter expression
    #[serde(default)]
    pub fhirpath_filter: Option<String>,
    /// Cron expression for cron triggers
    #[serde(default)]
    pub cron_expression: Option<String>,
}

/// Request to update an existing automation
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAutomation {
    /// Human-readable name (optional update)
    #[serde(default)]
    pub name: Option<String>,
    /// Description (optional update)
    #[serde(default)]
    pub description: Option<String>,
    /// JavaScript source code (optional update)
    #[serde(default)]
    pub source_code: Option<String>,
    /// Automation status (optional update)
    #[serde(default)]
    pub status: Option<AutomationStatus>,
    /// Maximum execution time in milliseconds (optional update)
    #[serde(default)]
    pub timeout_ms: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_automation_status_serialization() {
        let status = AutomationStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""active""#);

        let deserialized: AutomationStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, AutomationStatus::Active);
    }

    #[test]
    fn test_trigger_type_serialization() {
        let trigger_type = AutomationTriggerType::ResourceEvent;
        let json = serde_json::to_string(&trigger_type).unwrap();
        assert_eq!(json, r#""resource_event""#);
    }

    #[test]
    fn test_create_automation_deserialization() {
        let json = r#"{
            "name": "Welcome Automation",
            "description": "Creates welcome tasks for new patients",
            "source_code": "console.log('Hello');",
            "triggers": [{
                "trigger_type": "resource_event",
                "resource_type": "Patient",
                "event_types": ["created"]
            }]
        }"#;

        let create_automation: CreateAutomation = serde_json::from_str(json).unwrap();
        assert_eq!(create_automation.name, "Welcome Automation");
        assert_eq!(create_automation.timeout_ms, 5000); // default
        assert_eq!(create_automation.triggers.len(), 1);
        assert_eq!(
            create_automation.triggers[0].trigger_type,
            AutomationTriggerType::ResourceEvent
        );
    }
}
