pub mod error;
pub mod events;
pub mod fhir;
pub mod fhir_reference;
pub mod id;
pub mod monitoring;
pub mod operations;
pub mod resource;
pub mod time;

pub use error::{CoreError, Result};
pub use fhir::{FhirVersion, ResourceType};
pub use fhir_reference::{FhirReference, UnresolvableReference, parse_reference};
pub use id::{IdError, generate_id, validate_id};
pub use monitoring::{
    HealthCheck, HealthStatus, MemoryStats, MetricsCollector, ResourceStats, SystemMetrics,
};
pub use operations::{AppReference, OperationDefinition, OperationProvider, categories, modules};
pub use resource::{ResourceEnvelope, ResourceMeta, ResourceStatus};
pub use time::{FhirDateTime, now_utc};
