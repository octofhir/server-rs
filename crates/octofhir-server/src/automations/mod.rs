//! Automation system for OctoFHIR.
//!
//! This module provides automation using JavaScriptCore.
//! Automations are JavaScript scripts that can be triggered by resource events,
//! cron schedules, or manual invocation.
//!
//! # Architecture
//!
//! ```text
//! Resource Event → AutomationDispatcherHook → Automation Matching → AutomationExecutor → JSC Runtime
//!                                                            ↓
//!                                                     fhir.* / http.* APIs
//! ```
//!
//! # Automation APIs
//!
//! Automations have access to the following APIs:
//!
//! - `fhir.create(resource)` - Create a FHIR resource
//! - `fhir.read(resourceType, id)` - Read a FHIR resource
//! - `fhir.update(resource)` - Update a FHIR resource
//! - `fhir.delete(resourceType, id)` - Delete a FHIR resource
//! - `fhir.search(resourceType, params)` - Search for resources
//! - `http.fetch(url, options)` - Make HTTP requests
//! - `console.log/warn/error(...)` - Logging
//!
//! # Example Automation
//!
//! ```javascript
//! // Trigger: Patient created
//! if (event.type === 'created' && event.resource.resourceType === 'Patient') {
//!   const patient = event.resource;
//!   fhir.create({
//!     resourceType: 'Task',
//!     status: 'requested',
//!     intent: 'order',
//!     description: `Welcome patient ${patient.name?.[0]?.text || 'Unknown'}`,
//!     for: { reference: `Patient/${patient.id}` }
//!   });
//!   console.log(`Created welcome task for Patient/${patient.id}`);
//! }
//! ```

mod dispatcher;
mod executor;
mod fhir_api;
mod fhir_client;
pub mod handlers;
mod scheduler;
mod storage;
mod types;

pub use dispatcher::AutomationDispatcherHook;
pub use executor::{AutomationExecutor, AutomationExecutorConfig};
pub use fhir_client::StorageFhirClient;
pub use handlers::{
    AutomationState, add_trigger, automation_routes, create_automation, delete_automation,
    delete_trigger, deploy_automation, execute_automation, get_automation, get_automation_logs,
    list_automations, update_automation,
};
pub use scheduler::{CronScheduler, SchedulerConfig};
pub use storage::{AutomationStorage, PostgresAutomationStorage};
pub use types::{
    Automation, AutomationExecution, AutomationExecutionStatus, AutomationStatus,
    AutomationTrigger, AutomationTriggerType, CreateAutomation, CreateAutomationTrigger,
    UpdateAutomation,
};
