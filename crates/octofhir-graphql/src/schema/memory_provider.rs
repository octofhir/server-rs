//! In-memory model provider for GraphQL schema building.
//!
//! This module provides `InMemoryModelProvider`, a model provider that holds
//! all FHIR schemas in memory. It's optimized for the GraphQL schema build
//! process where all schemas need to be accessed.
//!
//! After the GraphQL schema is built, this provider can be dropped to free
//! memory, as the built GraphQL schema is self-contained.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use octofhir_fhir_model::error::Result as ModelResult;
use octofhir_fhir_model::provider::{
    ChoiceTypeInfo, ElementInfo, FhirVersion, ModelProvider, TypeInfo,
};
use octofhir_fhirschema::types::FhirSchema;
use tracing::debug;

/// FHIR to FHIRPath type mapping
const TYPE_MAPPING: &[(&str, &str)] = &[
    ("boolean", "Boolean"),
    ("integer", "Integer"),
    ("string", "String"),
    ("decimal", "Decimal"),
    ("uri", "String"),
    ("url", "String"),
    ("canonical", "String"),
    ("base64Binary", "String"),
    ("instant", "DateTime"),
    ("date", "Date"),
    ("dateTime", "DateTime"),
    ("time", "Time"),
    ("code", "String"),
    ("oid", "String"),
    ("id", "String"),
    ("markdown", "String"),
    ("unsignedInt", "Integer"),
    ("positiveInt", "Integer"),
    ("uuid", "String"),
    ("xhtml", "String"),
    ("Quantity", "Quantity"),
    ("SimpleQuantity", "Quantity"),
    ("Money", "Quantity"),
    ("Duration", "Quantity"),
    ("Age", "Quantity"),
    ("Distance", "Quantity"),
    ("Count", "Quantity"),
    ("Any", "Any"),
];

/// In-memory model provider for GraphQL schema building.
///
/// Holds all FHIR schemas in memory for fast access during GraphQL schema
/// generation. This is more efficient than on-demand loading when all schemas
/// need to be processed.
///
/// # Memory Management
///
/// After the GraphQL schema is built, this provider should be dropped to free
/// the schema memory. The built GraphQL schema is self-contained and doesn't
/// need the original FHIR schemas.
///
/// # Example
///
/// ```ignore
/// // Load all schemas from database
/// let records = store.bulk_load_fhirschemas_for_graphql("R4").await?;
///
/// // Create provider from records
/// let provider = InMemoryModelProvider::from_records(records, FhirVersion::R4)?;
///
/// // Build GraphQL schema
/// let builder = FhirSchemaBuilder::new(search_registry, Arc::new(provider), config);
/// let schema = builder.build().await?;
///
/// // Provider is dropped here, freeing memory
/// ```
#[derive(Debug)]
pub struct InMemoryModelProvider {
    /// Schemas indexed by name (e.g., "Patient", "Observation")
    schemas_by_name: HashMap<String, Arc<FhirSchema>>,
    /// Resource type names (schemas with kind = "resource" or "logical")
    resource_types: Vec<String>,
    /// Complex type names (schemas with kind = "complex-type")
    complex_types: Vec<String>,
    /// FHIR version
    fhir_version: FhirVersion,
    /// Type mapping for FHIR to FHIRPath conversion
    type_mapping: HashMap<String, String>,
}

impl InMemoryModelProvider {
    /// Create a new in-memory provider from pre-loaded FHIRSchema records.
    ///
    /// # Arguments
    /// * `schemas` - Pre-loaded and deserialized FHIRSchemas
    /// * `fhir_version` - FHIR version these schemas belong to
    pub fn new(schemas: Vec<FhirSchema>, fhir_version: FhirVersion) -> Self {
        let type_mapping: HashMap<String, String> = TYPE_MAPPING
            .iter()
            .map(|(fhir, fhirpath)| (fhir.to_string(), fhirpath.to_string()))
            .collect();

        let mut schemas_by_name = HashMap::new();
        let mut resource_types = Vec::new();
        let mut complex_types = Vec::new();

        for schema in schemas {
            let name = schema.name.clone();
            let kind = schema.kind.clone();

            schemas_by_name.insert(name.clone(), Arc::new(schema));

            match kind.as_str() {
                "resource" | "logical" => {
                    resource_types.push(name);
                }
                "complex-type" => {
                    complex_types.push(name);
                }
                _ => {}
            }
        }

        resource_types.sort();
        complex_types.sort();

        debug!(
            resource_count = resource_types.len(),
            complex_count = complex_types.len(),
            "InMemoryModelProvider initialized"
        );

        Self {
            schemas_by_name,
            resource_types,
            complex_types,
            fhir_version,
            type_mapping,
        }
    }

    /// Get a schema by name.
    pub fn get_schema(&self, name: &str) -> Option<Arc<FhirSchema>> {
        self.schemas_by_name.get(name).cloned()
    }

    /// Map FHIR type to FHIRPath type.
    fn map_fhir_type(&self, fhir_type: &str) -> String {
        self.type_mapping
            .get(fhir_type)
            .cloned()
            .unwrap_or_else(|| fhir_type.to_string())
    }

    /// Get backbone element's nested elements by parent type and path.
    fn get_backbone_elements_by_path(
        &self,
        parent_type: &str,
        element_path: &str,
    ) -> Option<HashMap<String, octofhir_fhirschema::types::FhirSchemaElement>> {
        let schema = self.get_schema(parent_type)?;
        let mut current_elements = schema.elements.as_ref()?.clone();

        for part in element_path.split('.') {
            let element = current_elements.get(part)?;
            current_elements = element.elements.as_ref()?.clone();
        }

        Some(current_elements)
    }

    /// Returns the total number of schemas loaded.
    pub fn schema_count(&self) -> usize {
        self.schemas_by_name.len()
    }
}

#[async_trait]
impl ModelProvider for InMemoryModelProvider {
    async fn get_type(&self, type_name: &str) -> ModelResult<Option<TypeInfo>> {
        if let Some(schema) = self.get_schema(type_name) {
            let mapped_type = if let Some(mapped) = self.type_mapping.get(&schema.name) {
                mapped.clone()
            } else if schema.kind == "resource" || schema.kind == "complex-type" {
                "Any".to_string()
            } else {
                self.map_fhir_type(&schema.name)
            };

            Ok(Some(TypeInfo {
                type_name: mapped_type,
                singleton: Some(true),
                is_empty: Some(false),
                namespace: Some("FHIR".to_string()),
                name: Some(schema.name.clone()),
            }))
        } else if let Some(mapped) = self.type_mapping.get(type_name) {
            Ok(Some(TypeInfo {
                type_name: mapped.clone(),
                singleton: Some(true),
                is_empty: Some(false),
                namespace: Some("System".to_string()),
                name: Some(type_name.to_string()),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_element_type(
        &self,
        parent_type: &TypeInfo,
        property_name: &str,
    ) -> ModelResult<Option<TypeInfo>> {
        if let Some(type_name) = &parent_type.name {
            let elements = if type_name.contains('.') {
                let parts: Vec<&str> = type_name.splitn(2, '.').collect();
                if parts.len() == 2 {
                    self.get_backbone_elements_by_path(parts[0], parts[1])
                } else {
                    None
                }
            } else {
                self.get_schema(type_name)
                    .and_then(|schema| schema.elements.clone())
            };

            let Some(elements) = elements else {
                return Ok(None);
            };

            // Try direct property name match
            if let Some(element) = elements.get(property_name) {
                if element.elements.is_some() {
                    let backbone_path = format!("{}.{}", type_name, property_name);
                    return Ok(Some(TypeInfo {
                        type_name: "Any".to_string(),
                        singleton: Some(element.max == Some(1)),
                        is_empty: Some(false),
                        namespace: Some("FHIR".to_string()),
                        name: Some(backbone_path),
                    }));
                }

                if let Some(element_type_name) = &element.type_name {
                    let mapped_type = self.map_fhir_type(element_type_name);
                    return Ok(Some(TypeInfo {
                        type_name: mapped_type,
                        singleton: Some(element.max == Some(1)),
                        is_empty: Some(false),
                        namespace: Some("FHIR".to_string()),
                        name: Some(element_type_name.clone()),
                    }));
                }
            }

            // Handle choice navigation
            for (element_name, element) in &elements {
                if element_name.ends_with("[x]") {
                    let base_name = element_name.trim_end_matches("[x]");
                    if let Some(type_suffix) = property_name.strip_prefix(base_name)
                        && !type_suffix.is_empty() {
                            let mut chars = type_suffix.chars();
                            if let Some(first_char) = chars.next() {
                                let schema_type =
                                    format!("{}{}", first_char.to_lowercase(), chars.as_str());

                                if let Some(choices) = &element.choices
                                    && choices.contains(&schema_type) {
                                        let mapped_type = self.map_fhir_type(&schema_type);
                                        return Ok(Some(TypeInfo {
                                            type_name: mapped_type,
                                            singleton: Some(element.max == Some(1)),
                                            is_empty: Some(false),
                                            namespace: if schema_type
                                                .chars()
                                                .next()
                                                .unwrap()
                                                .is_uppercase()
                                            {
                                                Some("FHIR".to_string())
                                            } else {
                                                Some("System".to_string())
                                            },
                                            name: Some(schema_type),
                                        }));
                                    }
                            }
                        }
                }
            }
        }
        Ok(None)
    }

    fn of_type(&self, type_info: &TypeInfo, target_type: &str) -> Option<TypeInfo> {
        if type_info.type_name == target_type {
            return Some(type_info.clone());
        }
        if let Some(ref name) = type_info.name
            && name == target_type {
                return Some(type_info.clone());
            }
        None
    }

    fn get_element_names(&self, parent_type: &TypeInfo) -> Vec<String> {
        if let Some(type_name) = &parent_type.name
            && let Some(schema) = self.get_schema(type_name)
                && let Some(elements) = &schema.elements {
                    return elements.keys().cloned().collect();
                }
        Vec::new()
    }

    async fn get_children_type(&self, parent_type: &TypeInfo) -> ModelResult<Option<TypeInfo>> {
        if parent_type.singleton.unwrap_or(true) {
            Ok(None)
        } else {
            Ok(Some(TypeInfo {
                type_name: parent_type.type_name.clone(),
                singleton: Some(true),
                is_empty: Some(false),
                namespace: parent_type.namespace.clone(),
                name: parent_type.name.clone(),
            }))
        }
    }

    async fn get_elements(&self, type_name: &str) -> ModelResult<Vec<ElementInfo>> {
        let mut element_infos = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        let mut current_type = Some(type_name.to_string());
        while let Some(ref type_to_check) = current_type {
            if let Some(schema) = self.get_schema(type_to_check) {
                if let Some(elements) = &schema.elements {
                    for (name, element) in elements {
                        if !seen_names.contains(name) {
                            seen_names.insert(name.clone());

                            let element_type = if element.elements.is_some() {
                                "BackboneElement".to_string()
                            } else {
                                element
                                    .type_name
                                    .as_ref()
                                    .unwrap_or(&"Any".to_string())
                                    .clone()
                            };

                            element_infos.push(ElementInfo {
                                name: name.clone(),
                                element_type,
                                documentation: element.short.clone(),
                            });
                        }
                    }
                }

                current_type = schema
                    .base
                    .as_ref()
                    .and_then(|base_url| base_url.rsplit('/').next().map(|s| s.to_string()));
            } else {
                current_type = None;
            }
        }

        Ok(element_infos)
    }

    async fn get_resource_types(&self) -> ModelResult<Vec<String>> {
        Ok(self.resource_types.clone())
    }

    async fn get_complex_types(&self) -> ModelResult<Vec<String>> {
        Ok(self.complex_types.clone())
    }

    async fn get_primitive_types(&self) -> ModelResult<Vec<String>> {
        let primitive_types: Vec<String> = self
            .type_mapping
            .keys()
            .filter(|&name| {
                !matches!(
                    name.as_str(),
                    "Quantity"
                        | "SimpleQuantity"
                        | "Money"
                        | "Duration"
                        | "Age"
                        | "Distance"
                        | "Count"
                        | "Any"
                )
            })
            .cloned()
            .collect();
        Ok(primitive_types)
    }

    async fn resource_type_exists(&self, resource_type: &str) -> ModelResult<bool> {
        Ok(self.get_schema(resource_type).is_some())
    }

    async fn get_fhir_version(&self) -> ModelResult<FhirVersion> {
        Ok(self.fhir_version.clone())
    }

    fn is_type_derived_from(&self, derived_type: &str, base_type: &str) -> bool {
        derived_type == base_type
    }

    async fn get_choice_types(
        &self,
        parent_type: &str,
        property_name: &str,
    ) -> ModelResult<Option<Vec<ChoiceTypeInfo>>> {
        if let Some(schema) = self.get_schema(parent_type)
            && let Some(elements) = &schema.elements {
                let choice_key = format!("{}[x]", property_name);
                if let Some(element) = elements.get(&choice_key)
                    && let Some(choices) = &element.choices {
                        let choice_infos: Vec<ChoiceTypeInfo> = choices
                            .iter()
                            .map(|type_name| {
                                let suffix = type_name
                                    .chars()
                                    .next()
                                    .map(|c| c.to_uppercase().to_string())
                                    .unwrap_or_default()
                                    + &type_name.chars().skip(1).collect::<String>();
                                ChoiceTypeInfo {
                                    suffix,
                                    type_name: type_name.clone(),
                                }
                            })
                            .collect();
                        return Ok(Some(choice_infos));
                    }
            }
        Ok(None)
    }

    async fn get_union_types(&self, _type_info: &TypeInfo) -> ModelResult<Option<Vec<TypeInfo>>> {
        Ok(None)
    }

    fn is_union_type(&self, _type_info: &TypeInfo) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_provider() {
        let provider = InMemoryModelProvider::new(Vec::new(), FhirVersion::R4);
        assert_eq!(provider.schema_count(), 0);
        assert!(provider.resource_types.is_empty());
        assert!(provider.complex_types.is_empty());
    }

    #[tokio::test]
    async fn test_get_resource_types() {
        let provider = InMemoryModelProvider::new(Vec::new(), FhirVersion::R4);
        let types = provider.get_resource_types().await.unwrap();
        assert!(types.is_empty());
    }
}
