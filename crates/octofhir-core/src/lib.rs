pub mod error;
pub mod fhir;
pub mod time;
pub mod id;
pub mod monitoring;
pub mod resource;

pub use error::{CoreError, Result};
pub use fhir::{ResourceType, FhirVersion};
pub use time::{FhirDateTime, now_utc};
pub use id::{generate_id, validate_id, IdError};
pub use monitoring::{SystemMetrics, ResourceStats, MemoryStats, HealthCheck, HealthStatus, MetricsCollector};
pub use resource::{ResourceEnvelope, ResourceMeta, ResourceStatus};
