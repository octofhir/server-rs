//! Server-wide model provider wrapper that extends FhirSchemaModelProvider
//! with server-specific methods (LSP, validation, etc.)

use async_trait::async_trait;
use octofhir_fhir_model::error::Result as ModelResult;
use octofhir_fhir_model::provider::{
    ChoiceTypeInfo, ElementInfo, FhirVersion, ModelProvider, TypeInfo,
};
use octofhir_fhirschema::FhirSchemaModelProvider;

/// Server-wide model provider that wraps FhirSchemaModelProvider
/// and adds server-specific methods for LSP, validation, etc.
#[derive(Debug)]
pub struct OctoFhirModelProvider {
    inner: FhirSchemaModelProvider,
}

impl OctoFhirModelProvider {
    /// Create new OctoFhir model provider wrapping a FhirSchemaModelProvider
    pub fn new(provider: FhirSchemaModelProvider) -> Self {
        Self { inner: provider }
    }

    /// Check if an element is a choice type variant and return the base element name.
    /// E.g., for Patient type and "deceasedBoolean" element, returns Some("deceased")
    /// Uses the same logic as FhirSchemaModelProvider.get_element_type()
    ///
    /// # Arguments
    /// * `type_name` - The FHIR resource or complex type name (e.g., "Patient")
    /// * `element_name` - The element name to check (e.g., "deceasedBoolean")
    ///
    /// # Returns
    /// * `Some(base_name)` if the element is a choice variant (e.g., "deceased")
    /// * `None` if the element is not a choice variant
    pub fn get_choice_base_name(&self, type_name: &str, element_name: &str) -> Option<String> {
        // Access schemas via the public accessor method
        let schemas = self.inner.schemas();
        let schema = schemas.get(type_name)?;
        let elements = schema.elements.as_ref()?;

        // In FhirSchema, choice type information is stored in the `choice_of` field
        // - Variant fields (e.g., "deceasedBoolean") have `choice_of: Some("deceased")`
        // - Base fields (e.g., "deceased") have `choices: Some([...])` and `choice_of: None`

        // Look up the element in the schema
        if let Some(element) = elements.get(element_name) {
            // If this element has a choice_of field, it's a choice variant
            if let Some(base_name) = &element.choice_of {
                return Some(base_name.clone());
            }
        }

        None
    }

    /// Debug helper to print choice type information for a type
    #[allow(dead_code)]
    pub fn debug_choice_types(&self, type_name: &str) {
        let schemas = self.inner.schemas();
        if let Some(schema) = schemas.get(type_name) {
            if let Some(elements) = &schema.elements {
                println!("\nAll elements in {} schema:", type_name);
                for (name, elem) in elements {
                    if name.contains("deceased") || name.contains("multipleBirth") {
                        println!("  {} ->", name);
                        println!("    type_name: {:?}", elem.type_name);
                        println!("    choices: {:?}", elem.choices);
                        println!("    choice_of: {:?}", elem.choice_of);
                    }
                }
            }
        }
    }
}

// Implement ModelProvider trait by delegating all methods to inner
#[async_trait]
impl ModelProvider for OctoFhirModelProvider {
    async fn get_type(&self, type_name: &str) -> ModelResult<Option<TypeInfo>> {
        self.inner.get_type(type_name).await
    }

    async fn get_element_type(
        &self,
        parent_type: &TypeInfo,
        property_name: &str,
    ) -> ModelResult<Option<TypeInfo>> {
        self.inner
            .get_element_type(parent_type, property_name)
            .await
    }

    fn of_type(&self, type_info: &TypeInfo, target_type: &str) -> Option<TypeInfo> {
        self.inner.of_type(type_info, target_type)
    }

    fn get_element_names(&self, parent_type: &TypeInfo) -> Vec<String> {
        self.inner.get_element_names(parent_type)
    }

    async fn get_children_type(&self, parent_type: &TypeInfo) -> ModelResult<Option<TypeInfo>> {
        self.inner.get_children_type(parent_type).await
    }

    async fn get_elements(&self, type_name: &str) -> ModelResult<Vec<ElementInfo>> {
        self.inner.get_elements(type_name).await
    }

    async fn get_resource_types(&self) -> ModelResult<Vec<String>> {
        self.inner.get_resource_types().await
    }

    async fn get_complex_types(&self) -> ModelResult<Vec<String>> {
        self.inner.get_complex_types().await
    }

    async fn get_primitive_types(&self) -> ModelResult<Vec<String>> {
        self.inner.get_primitive_types().await
    }

    async fn resource_type_exists(&self, resource_type: &str) -> ModelResult<bool> {
        self.inner.resource_type_exists(resource_type).await
    }

    async fn get_fhir_version(&self) -> ModelResult<FhirVersion> {
        self.inner.get_fhir_version().await
    }

    fn is_type_derived_from(&self, derived_type: &str, base_type: &str) -> bool {
        self.inner.is_type_derived_from(derived_type, base_type)
    }

    async fn get_choice_types(
        &self,
        parent_type: &str,
        property_name: &str,
    ) -> ModelResult<Option<Vec<ChoiceTypeInfo>>> {
        self.inner
            .get_choice_types(parent_type, property_name)
            .await
    }

    async fn get_union_types(&self, type_info: &TypeInfo) -> ModelResult<Option<Vec<TypeInfo>>> {
        self.inner.get_union_types(type_info).await
    }

    fn is_union_type(&self, type_info: &TypeInfo) -> bool {
        self.inner.is_union_type(type_info)
    }
}
