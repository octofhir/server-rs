pub mod error;
pub mod events;
pub mod fhir;
pub mod fhir_reference;
pub mod id;
pub mod monitoring;
pub mod operations;
pub mod resource;
pub mod search_index;
pub mod time;

pub use error::{CoreError, Result};
pub use fhir::{FhirVersion, ResourceType};
pub use fhir_reference::{
    FhirReference, NormalizedRef, UnresolvableReference, normalize_reference_for_index,
    normalize_reference_string, parse_reference, ref_kind,
};
pub use id::{IdError, generate_id, validate_id};
pub use monitoring::{
    HealthCheck, HealthStatus, MemoryStats, MetricsCollector, ResourceStats, SystemMetrics,
};
pub use operations::{AppReference, OperationDefinition, OperationProvider, categories, modules};
pub use resource::{ResourceEnvelope, ResourceMeta, ResourceStatus};
pub use time::{FhirDateTime, now_utc};
