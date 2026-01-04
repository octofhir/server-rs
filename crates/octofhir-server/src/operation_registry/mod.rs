//! Operation Registry Module
//!
//! This module provides a registry for tracking all server operations.
//! Operations are defined by modules (FHIR, GraphQL, Auth, UI, etc.) and
//! stored in the database for UI display and policy targeting.

mod providers;
mod registry;
mod storage;

pub use providers::{
    AuthOperationProvider, FhirOperationProvider, GatewayOperationProvider,
    NotificationsOperationProvider, SystemOperationProvider, UiOperationProvider,
};
pub use registry::OperationRegistryService;
pub use storage::{OperationStorage, OperationUpdate, PostgresOperationStorage};
