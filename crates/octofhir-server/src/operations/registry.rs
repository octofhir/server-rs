//! Operation registry for storing and looking up FHIR operations.
//!
//! The registry indexes operations by code, URL, and the level at which
//! they can be invoked (system, type, or instance level).

use std::collections::HashMap;
use std::sync::Arc;

use super::definition::OperationDefinition;

/// Registry for FHIR operations.
///
/// Provides efficient lookup of operations by code, URL, and operation level.
/// Operations are stored as `Arc<OperationDefinition>` to allow sharing
/// across multiple indices.
#[derive(Debug, Default)]
pub struct OperationRegistry {
    /// Operations indexed by code (e.g., "validate", "meta")
    by_code: HashMap<String, Arc<OperationDefinition>>,
    /// Operations indexed by canonical URL
    by_url: HashMap<String, Arc<OperationDefinition>>,
    /// System-level operations
    system_ops: Vec<Arc<OperationDefinition>>,
    /// Type-level operations indexed by resource type
    type_ops: HashMap<String, Vec<Arc<OperationDefinition>>>,
    /// Instance-level operations indexed by resource type
    instance_ops: HashMap<String, Vec<Arc<OperationDefinition>>>,
}

impl OperationRegistry {
    /// Creates a new empty operation registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an operation definition in the registry.
    ///
    /// The operation will be indexed by code, URL, and the appropriate
    /// operation levels based on its configuration.
    pub fn register(&mut self, op: OperationDefinition) {
        let op = Arc::new(op);

        self.by_code.insert(op.code.clone(), Arc::clone(&op));

        if !op.url.is_empty() {
            self.by_url.insert(op.url.clone(), Arc::clone(&op));
        }

        if op.system {
            self.system_ops.push(Arc::clone(&op));
        }

        if op.type_level {
            if op.resource.is_empty() {
                // Operation applies to all resource types
                self.type_ops
                    .entry("Resource".to_string())
                    .or_default()
                    .push(Arc::clone(&op));
            } else {
                for resource in &op.resource {
                    self.type_ops
                        .entry(resource.clone())
                        .or_default()
                        .push(Arc::clone(&op));
                }
            }
        }

        if op.instance {
            if op.resource.is_empty() {
                // Operation applies to all resource types
                self.instance_ops
                    .entry("Resource".to_string())
                    .or_default()
                    .push(Arc::clone(&op));
            } else {
                for resource in &op.resource {
                    self.instance_ops
                        .entry(resource.clone())
                        .or_default()
                        .push(Arc::clone(&op));
                }
            }
        }
    }

    /// Gets an operation by its code.
    pub fn get_by_code(&self, code: &str) -> Option<Arc<OperationDefinition>> {
        self.by_code.get(code).cloned()
    }

    /// Gets an operation by its canonical URL.
    pub fn get_by_url(&self, url: &str) -> Option<Arc<OperationDefinition>> {
        self.by_url.get(url).cloned()
    }

    /// Gets a system-level operation by code.
    pub fn get_system_operation(&self, code: &str) -> Option<Arc<OperationDefinition>> {
        self.system_ops.iter().find(|op| op.code == code).cloned()
    }

    /// Gets a type-level operation by resource type and code.
    ///
    /// Falls back to checking operations defined for "Resource" (all types)
    /// if no type-specific operation is found.
    pub fn get_type_operation(
        &self,
        resource_type: &str,
        code: &str,
    ) -> Option<Arc<OperationDefinition>> {
        // First check type-specific operations
        if let Some(op) = self
            .type_ops
            .get(resource_type)
            .and_then(|ops| ops.iter().find(|op| op.code == code).cloned())
        {
            return Some(op);
        }

        // Fall back to generic Resource operations
        self.type_ops
            .get("Resource")
            .and_then(|ops| ops.iter().find(|op| op.code == code).cloned())
    }

    /// Gets an instance-level operation by resource type and code.
    ///
    /// Falls back to checking operations defined for "Resource" (all types)
    /// if no type-specific operation is found.
    pub fn get_instance_operation(
        &self,
        resource_type: &str,
        code: &str,
    ) -> Option<Arc<OperationDefinition>> {
        // First check type-specific operations
        if let Some(op) = self
            .instance_ops
            .get(resource_type)
            .and_then(|ops| ops.iter().find(|op| op.code == code).cloned())
        {
            return Some(op);
        }

        // Fall back to generic Resource operations
        self.instance_ops
            .get("Resource")
            .and_then(|ops| ops.iter().find(|op| op.code == code).cloned())
    }

    /// Returns all system-level operations.
    pub fn system_operations(&self) -> &[Arc<OperationDefinition>] {
        &self.system_ops
    }

    /// Returns all type-level operations for a given resource type.
    pub fn type_operations(&self, resource_type: &str) -> Vec<Arc<OperationDefinition>> {
        let mut ops = self
            .type_ops
            .get(resource_type)
            .cloned()
            .unwrap_or_default();

        // Include generic Resource operations
        if let Some(generic_ops) = self.type_ops.get("Resource") {
            for op in generic_ops {
                if !ops.iter().any(|o| o.code == op.code) {
                    ops.push(Arc::clone(op));
                }
            }
        }

        ops
    }

    /// Returns all instance-level operations for a given resource type.
    pub fn instance_operations(&self, resource_type: &str) -> Vec<Arc<OperationDefinition>> {
        let mut ops = self
            .instance_ops
            .get(resource_type)
            .cloned()
            .unwrap_or_default();

        // Include generic Resource operations
        if let Some(generic_ops) = self.instance_ops.get("Resource") {
            for op in generic_ops {
                if !ops.iter().any(|o| o.code == op.code) {
                    ops.push(Arc::clone(op));
                }
            }
        }

        ops
    }

    /// Returns the total number of registered operations.
    pub fn len(&self) -> usize {
        self.by_code.len()
    }

    /// Returns all registered operations (deduplicated by code).
    pub fn all(&self) -> Vec<Arc<OperationDefinition>> {
        self.by_code.values().cloned().collect()
    }

    /// Returns true if no operations are registered.
    pub fn is_empty(&self) -> bool {
        self.by_code.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::definition::OperationKind;

    fn create_test_operation(
        code: &str,
        system: bool,
        type_level: bool,
        instance: bool,
        resources: Vec<&str>,
    ) -> OperationDefinition {
        OperationDefinition {
            code: code.to_string(),
            url: format!("http://hl7.org/fhir/OperationDefinition/{}", code),
            kind: OperationKind::Operation,
            system,
            type_level,
            instance,
            resource: resources.into_iter().map(String::from).collect(),
            parameters: vec![],
            affects_state: false,
        }
    }

    #[test]
    fn test_register_and_lookup_by_code() {
        let mut registry = OperationRegistry::new();
        registry.register(create_test_operation(
            "validate",
            false,
            true,
            true,
            vec!["Patient"],
        ));

        assert!(registry.get_by_code("validate").is_some());
        assert!(registry.get_by_code("unknown").is_none());
    }

    #[test]
    fn test_system_operation() {
        let mut registry = OperationRegistry::new();
        registry.register(create_test_operation("meta", true, false, false, vec![]));

        assert!(registry.get_system_operation("meta").is_some());
        assert!(registry.get_system_operation("validate").is_none());
    }

    #[test]
    fn test_type_operation() {
        let mut registry = OperationRegistry::new();
        registry.register(create_test_operation(
            "validate",
            false,
            true,
            false,
            vec!["Patient", "Observation"],
        ));

        assert!(registry.get_type_operation("Patient", "validate").is_some());
        assert!(
            registry
                .get_type_operation("Observation", "validate")
                .is_some()
        );
        assert!(
            registry
                .get_type_operation("Encounter", "validate")
                .is_none()
        );
    }

    #[test]
    fn test_instance_operation() {
        let mut registry = OperationRegistry::new();
        registry.register(create_test_operation(
            "meta",
            false,
            false,
            true,
            vec!["Patient"],
        ));

        assert!(registry.get_instance_operation("Patient", "meta").is_some());
        assert!(
            registry
                .get_instance_operation("Observation", "meta")
                .is_none()
        );
    }

    #[test]
    fn test_generic_resource_operation() {
        let mut registry = OperationRegistry::new();
        // Operation with empty resource list applies to all types
        registry.register(create_test_operation("validate", false, true, true, vec![]));

        // Should be available for any resource type
        assert!(registry.get_type_operation("Patient", "validate").is_some());
        assert!(
            registry
                .get_type_operation("Observation", "validate")
                .is_some()
        );
        assert!(
            registry
                .get_instance_operation("Patient", "validate")
                .is_some()
        );
    }
}
