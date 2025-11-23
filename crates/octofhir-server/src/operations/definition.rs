//! Core types for FHIR OperationDefinition resources.
//!
//! This module defines the Rust representations of FHIR OperationDefinition
//! resources, which describe the inputs, outputs, and behavior of FHIR operations.

/// Represents a FHIR OperationDefinition resource.
///
/// OperationDefinition describes a particular operation that can be invoked
/// on a FHIR server, including its parameters, supported resource types,
/// and the levels at which it can be invoked.
#[derive(Debug, Clone)]
pub struct OperationDefinition {
    /// The code used to invoke this operation (e.g., "validate", "meta")
    pub code: String,
    /// The canonical URL for this operation
    pub url: String,
    /// The kind of operation (operation vs query)
    pub kind: OperationKind,
    /// Whether this operation can be invoked at the system level
    pub system: bool,
    /// Whether this operation can be invoked at the type level
    pub type_level: bool,
    /// Whether this operation can be invoked at the instance level
    pub instance: bool,
    /// The resource types this operation applies to
    pub resource: Vec<String>,
    /// The parameters defined for this operation
    pub parameters: Vec<OperationParameter>,
    /// Whether this operation affects server state
    pub affects_state: bool,
}

/// The kind of FHIR operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    /// A standard FHIR operation
    Operation,
    /// A query operation (returns search results)
    Query,
}

/// Represents a parameter defined in an OperationDefinition.
#[derive(Debug, Clone)]
pub struct OperationParameter {
    /// The name of the parameter
    pub name: String,
    /// Whether this is an input or output parameter
    pub use_: ParameterUse,
    /// Minimum cardinality
    pub min: u32,
    /// Maximum cardinality ("*" for unlimited)
    pub max: String,
    /// The FHIR type of the parameter value
    pub param_type: Option<String>,
    /// For search parameters, the search type
    pub search_type: Option<String>,
    /// Target profiles for Reference parameters
    pub target_profile: Vec<String>,
    /// Nested parts for complex parameters
    pub parts: Vec<OperationParameter>,
}

/// Whether an operation parameter is for input or output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterUse {
    /// Input parameter
    In,
    /// Output parameter
    Out,
}
