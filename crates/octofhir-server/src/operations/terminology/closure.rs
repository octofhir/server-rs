//! CodeSystem $closure Operation
//!
//! Implements the FHIR CodeSystem/$closure operation to maintain a client-side
//! transitive closure table for concept hierarchies.
//!
//! Specification: http://hl7.org/fhir/codesystem-operation-closure.html
//!
//! The closure table is used to efficiently query ancestor/descendant relationships
//! in hierarchical code systems like SNOMED CT, ICD-10, etc.
//!
//! Supported invocation levels:
//! - System: `POST /$closure` with name and optional concept

use async_trait::async_trait;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

/// Global storage for closure tables.
/// Each closure table is identified by a unique name and stores the transitive closure.
static CLOSURE_TABLES: LazyLock<DashMap<String, ClosureTable>> = LazyLock::new(DashMap::new);

/// A closure table storing transitive relationships between concepts.
#[derive(Debug, Clone, Default)]
pub struct ClosureTable {
    /// Version counter for the closure table
    pub version: u64,
    /// All concepts in the closure (code -> system)
    pub concepts: HashMap<String, String>,
    /// Subsumption relationships: parent -> set of children
    pub subsumes: HashMap<String, HashSet<String>>,
    /// Reverse relationships: child -> set of parents
    pub subsumed_by: HashMap<String, HashSet<String>>,
}

impl ClosureTable {
    /// Create a new empty closure table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a concept to the closure table with its relationships.
    pub fn add_concept(&mut self, code: &str, system: &str, parents: Vec<String>) {
        self.version += 1;
        self.concepts.insert(code.to_string(), system.to_string());

        // Add subsumption relationships
        for parent in parents {
            self.subsumes
                .entry(parent.clone())
                .or_default()
                .insert(code.to_string());

            self.subsumed_by
                .entry(code.to_string())
                .or_default()
                .insert(parent);
        }
    }

    /// Get the transitive closure for a concept (all ancestors).
    pub fn get_ancestors(&self, code: &str) -> HashSet<String> {
        let mut ancestors = HashSet::new();
        let mut to_visit = vec![code.to_string()];
        let mut visited = HashSet::new();

        while let Some(current) = to_visit.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if let Some(parents) = self.subsumed_by.get(&current) {
                for parent in parents {
                    ancestors.insert(parent.clone());
                    to_visit.push(parent.clone());
                }
            }
        }

        ancestors
    }

    /// Get all descendants of a concept.
    pub fn get_descendants(&self, code: &str) -> HashSet<String> {
        let mut descendants = HashSet::new();
        let mut to_visit = vec![code.to_string()];
        let mut visited = HashSet::new();

        while let Some(current) = to_visit.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if let Some(children) = self.subsumes.get(&current) {
                for child in children {
                    descendants.insert(child.clone());
                    to_visit.push(child.clone());
                }
            }
        }

        descendants
    }
}

/// Parameters for the $closure operation.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosureParams {
    /// The name that defines the closure table
    pub name: Option<String>,

    /// Concepts to add to the closure table (array of Coding)
    #[serde(default)]
    pub concept: Vec<Value>,

    /// The version of the code system
    pub version: Option<String>,
}

/// The $closure operation handler.
pub struct ClosureOperation;

impl ClosureOperation {
    pub fn new() -> Self {
        Self
    }

    /// Extract parameters from a FHIR Parameters resource or query params.
    fn extract_params(params: &Value) -> Result<ClosureParams, OperationError> {
        if params.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
            let mut closure_params = ClosureParams::default();

            if let Some(parameters) = params.get("parameter").and_then(|v| v.as_array()) {
                for param in parameters {
                    let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    match name {
                        "name" => {
                            closure_params.name = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        "concept" => {
                            if let Some(coding) = param.get("valueCoding") {
                                closure_params.concept.push(coding.clone());
                            }
                        }
                        "version" => {
                            closure_params.version = param
                                .get("valueString")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        _ => {}
                    }
                }
            }

            Ok(closure_params)
        } else {
            // For query params, name is the only valid parameter
            let closure_params: ClosureParams =
                serde_json::from_value(params.clone()).unwrap_or_default();
            Ok(closure_params)
        }
    }

    /// Initialize a new closure table.
    fn initialize_closure(name: &str) -> Value {
        let table = ClosureTable::new();
        CLOSURE_TABLES.insert(name.to_string(), table);

        json!({
            "resourceType": "ConceptMap",
            "name": name,
            "status": "active",
            "experimental": true,
            "description": format!("Closure table '{}'", name),
            "group": []
        })
    }

    /// Add concepts to an existing closure table.
    async fn add_concepts_to_closure(
        &self,
        state: &AppState,
        name: &str,
        concepts: &[Value],
    ) -> Result<Value, OperationError> {
        let mut table = CLOSURE_TABLES
            .get(name)
            .map(|t| t.clone())
            .unwrap_or_else(ClosureTable::new);

        let mut new_mappings: Vec<Value> = Vec::new();

        for concept in concepts {
            let code = concept
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    OperationError::InvalidParameters("Concept must have a code".into())
                })?;
            let system = concept
                .get("system")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    OperationError::InvalidParameters("Concept must have a system".into())
                })?;

            // Skip if concept already exists
            if table.concepts.contains_key(code) {
                continue;
            }

            // Find parents using $subsumes or hierarchy
            let parents = self.find_parents(state, system, code).await?;

            // Add to closure table
            table.add_concept(code, system, parents.clone());

            // Generate mappings for the new concept
            for parent in &parents {
                new_mappings.push(json!({
                    "source": system,
                    "target": system,
                    "element": [{
                        "code": parent,
                        "target": [{
                            "code": code,
                            "equivalence": "subsumes"
                        }]
                    }]
                }));
            }

            // Add transitive mappings (grandparents, etc.)
            let ancestors = table.get_ancestors(code);
            for ancestor in &ancestors {
                if !parents.contains(ancestor) {
                    new_mappings.push(json!({
                        "source": system,
                        "target": system,
                        "element": [{
                            "code": ancestor,
                            "target": [{
                                "code": code,
                                "equivalence": "subsumes"
                            }]
                        }]
                    }));
                }
            }
        }

        // Store updated table
        CLOSURE_TABLES.insert(name.to_string(), table.clone());

        // Return ConceptMap with new mappings
        Ok(json!({
            "resourceType": "ConceptMap",
            "name": name,
            "status": "active",
            "version": table.version.to_string(),
            "group": new_mappings
        }))
    }

    /// Find parent concepts for a given code.
    async fn find_parents(
        &self,
        _state: &AppState,
        _system: &str,
        _code: &str,
    ) -> Result<Vec<String>, OperationError> {
        // In a full implementation, this would:
        // 1. Load the CodeSystem
        // 2. Find the concept's parent(s) in the hierarchy
        // 3. For SNOMED CT, use ECL queries or external terminology server
        //
        // For now, return empty parents (concept has no known parents)
        // Users can manually specify relationships by calling $closure multiple times
        Ok(Vec::new())
    }
}

impl Default for ClosureOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationHandler for ClosureOperation {
    fn code(&self) -> &str {
        "closure"
    }

    /// Handle system-level $closure.
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let closure_params = Self::extract_params(params)?;

        let name = closure_params.name.ok_or_else(|| {
            OperationError::InvalidParameters("The 'name' parameter is required".into())
        })?;

        // If no concepts provided, this is an initialization request
        if closure_params.concept.is_empty() {
            return Ok(Self::initialize_closure(&name));
        }

        // Add concepts to the closure table
        self.add_concepts_to_closure(state, &name, &closure_params.concept)
            .await
    }

    /// Handle type-level $closure (not typically used).
    async fn handle_type(
        &self,
        _state: &AppState,
        resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(format!(
            "$closure is a system-level operation, not supported on {}",
            resource_type
        )))
    }

    /// Handle instance-level $closure (not supported).
    async fn handle_instance(
        &self,
        _state: &AppState,
        resource_type: &str,
        _id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(format!(
            "$closure is a system-level operation, not supported on {} instances",
            resource_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closure_table_basic() {
        let mut table = ClosureTable::new();

        // Add concepts with hierarchy: A -> B -> C
        table.add_concept("B", "http://example.org", vec!["A".to_string()]);
        table.add_concept("C", "http://example.org", vec!["B".to_string()]);

        assert_eq!(table.concepts.len(), 2);
        assert_eq!(table.version, 2);

        // Check ancestors of C
        let ancestors = table.get_ancestors("C");
        assert!(ancestors.contains("B"));
        assert!(ancestors.contains("A"));

        // Check descendants of A
        let descendants = table.get_descendants("A");
        assert!(descendants.contains("B"));
        assert!(descendants.contains("C"));
    }

    #[test]
    fn test_closure_table_multiple_parents() {
        let mut table = ClosureTable::new();

        // Diamond hierarchy: A, B -> C (C has two parents)
        table.add_concept(
            "C",
            "http://example.org",
            vec!["A".to_string(), "B".to_string()],
        );

        let ancestors = table.get_ancestors("C");
        assert!(ancestors.contains("A"));
        assert!(ancestors.contains("B"));
    }

    #[test]
    fn test_extract_params_from_parameters() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "name", "valueString": "test-closure"},
                {"name": "concept", "valueCoding": {"system": "http://example.org", "code": "A"}},
                {"name": "concept", "valueCoding": {"system": "http://example.org", "code": "B"}}
            ]
        });

        let closure_params = ClosureOperation::extract_params(&params).unwrap();
        assert_eq!(closure_params.name, Some("test-closure".to_string()));
        assert_eq!(closure_params.concept.len(), 2);
    }

    #[test]
    fn test_initialize_closure() {
        let result = ClosureOperation::initialize_closure("test-table");

        assert_eq!(result["resourceType"], "ConceptMap");
        assert_eq!(result["name"], "test-table");
        assert_eq!(result["status"], "active");
    }
}
