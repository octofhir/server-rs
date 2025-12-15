//! FHIR type generator for GraphQL schema.
//!
//! This module generates GraphQL Object types from FHIR schema definitions,
//! including resources, complex types, and backbone elements.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use async_graphql::Value;
use async_graphql::dynamic::{Field, FieldFuture, Object, TypeRef};
use octofhir_fhir_model::provider::ModelProvider;
use tracing::{debug, trace};

use super::resource_type::fhir_type_to_graphql;
use crate::error::GraphQLError;

/// Registry for tracking generated GraphQL types.
///
/// Prevents duplicate type generation and handles circular references.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    /// Types that have been fully generated.
    generated: HashSet<String>,
    /// Types currently being generated (for cycle detection).
    generating: HashSet<String>,
    /// Types queued for generation.
    pending: VecDeque<String>,
    /// Generated Object types ready for registration.
    objects: HashMap<String, Object>,
}

impl TypeRegistry {
    /// Creates a new empty type registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks if a type has been generated or is being generated.
    pub fn is_known(&self, type_name: &str) -> bool {
        self.generated.contains(type_name) || self.generating.contains(type_name)
    }

    /// Marks a type as currently being generated.
    pub fn start_generating(&mut self, type_name: &str) {
        self.generating.insert(type_name.to_string());
    }

    /// Marks a type as fully generated and stores its Object.
    pub fn finish_generating(&mut self, type_name: &str, obj: Object) {
        self.generating.remove(type_name);
        self.generated.insert(type_name.to_string());
        self.objects.insert(type_name.to_string(), obj);
    }

    /// Queues a type for generation if not already known.
    pub fn queue_if_needed(&mut self, type_name: &str) {
        if !self.is_known(type_name) && !self.pending.contains(&type_name.to_string()) {
            self.pending.push_back(type_name.to_string());
        }
    }

    /// Gets the next type to generate.
    pub fn pop_pending(&mut self) -> Option<String> {
        self.pending.pop_front()
    }

    /// Takes all generated objects for registration.
    pub fn take_objects(&mut self) -> HashMap<String, Object> {
        std::mem::take(&mut self.objects)
    }

    /// Returns the number of generated types.
    pub fn generated_count(&self) -> usize {
        self.generated.len()
    }
}

/// Dynamic model provider type alias.
pub type DynModelProvider = Arc<dyn ModelProvider + Send + Sync>;

/// Generator for FHIR GraphQL types.
///
/// Uses the ModelProvider to introspect FHIR schemas and generate
/// corresponding GraphQL Object types. Reuses the same provider
/// used for validation, FHIRPath, and LSP.
pub struct FhirTypeGenerator {
    /// Model provider for accessing FHIR schema information.
    model_provider: DynModelProvider,
    /// Type registry for tracking generation.
    registry: TypeRegistry,
}

impl FhirTypeGenerator {
    /// Creates a new type generator.
    ///
    /// The `model_provider` should be the same instance used elsewhere
    /// in the server (e.g., `OctoFhirModelProvider`).
    pub fn new(model_provider: DynModelProvider) -> Self {
        Self {
            model_provider,
            registry: TypeRegistry::new(),
        }
    }

    /// Queues additional types for generation.
    ///
    /// This is useful when there are types needed that aren't automatically
    /// discovered from the model provider (e.g., resource types referenced
    /// by search parameters).
    pub fn queue_types(&mut self, types: impl IntoIterator<Item = impl AsRef<str>>) {
        for type_name in types {
            self.registry.queue_if_needed(type_name.as_ref());
        }
    }

    /// Generates all FHIR types (resources and complex types).
    ///
    /// Returns a map of type name to GraphQL Object.
    pub async fn generate_all_types(&mut self) -> Result<HashMap<String, Object>, GraphQLError> {
        // Queue all resource types
        let resource_types = self
            .model_provider
            .get_resource_types()
            .await
            .map_err(|e| {
                GraphQLError::SchemaBuildFailed(format!("Failed to get resource types: {}", e))
            })?;

        debug!(
            count = resource_types.len(),
            "Queueing resource types for generation"
        );
        let mut skipped_resources = Vec::new();
        for rt in &resource_types {
            // Skip types with invalid GraphQL names (e.g., profiles with hyphens)
            if is_valid_graphql_name(rt) {
                self.registry.queue_if_needed(rt);
            } else {
                skipped_resources.push(rt.as_str());
            }
        }

        if !skipped_resources.is_empty() {
            debug!(
                count = skipped_resources.len(),
                types = ?skipped_resources,
                "Skipped resource types with invalid GraphQL names (likely profiles)"
            );
        }

        // Queue all complex types
        let complex_types = self.model_provider.get_complex_types().await.map_err(|e| {
            GraphQLError::SchemaBuildFailed(format!("Failed to get complex types: {}", e))
        })?;

        debug!(
            count = complex_types.len(),
            "Queueing complex types for generation"
        );
        let mut skipped_complex = Vec::new();
        for ct in &complex_types {
            // Skip types with invalid GraphQL names (e.g., profiles with hyphens)
            if is_valid_graphql_name(ct) {
                self.registry.queue_if_needed(ct);
            } else {
                skipped_complex.push(ct.as_str());
            }
        }

        if !skipped_complex.is_empty() {
            debug!(
                count = skipped_complex.len(),
                types = ?skipped_complex,
                "Skipped complex types with invalid GraphQL names (likely profiles)"
            );
        }

        // Process queue until empty
        while let Some(type_name) = self.registry.pop_pending() {
            self.generate_type(&type_name).await?;
        }

        debug!(
            count = self.registry.generated_count(),
            "Type generation complete"
        );

        Ok(self.registry.take_objects())
    }

    /// Generates a single FHIR type.
    async fn generate_type(&mut self, type_name: &str) -> Result<(), GraphQLError> {
        // Skip if already generated or generating (cycle)
        if self.registry.is_known(type_name) {
            return Ok(());
        }

        // Skip primitive types - they use scalars
        if is_fhir_primitive(type_name) {
            return Ok(());
        }

        trace!(type_name = %type_name, "Generating GraphQL type");
        self.registry.start_generating(type_name);

        // Get elements for this type
        let elements = self
            .model_provider
            .get_elements(type_name)
            .await
            .map_err(|e| {
                GraphQLError::SchemaBuildFailed(format!(
                    "Failed to get elements for {}: {}",
                    type_name, e
                ))
            })?;

        // Create the Object type
        let mut obj = Object::new(type_name).description(format!("FHIR {} type", type_name));

        // Track choice base fields to skip
        let mut choice_bases: HashSet<String> = HashSet::new();

        // First pass: identify choice type base fields by looking for expanded variants
        for elem in &elements {
            // If this element has a choice_of reference, record the base
            if let Some(choices) = self.get_choice_types(type_name, &elem.name).await {
                if !choices.is_empty() {
                    choice_bases.insert(elem.name.clone());
                }
            }
        }

        // Track if we added any fields
        let mut has_fields = false;

        // Generate fields for each element
        for elem in &elements {
            // Skip choice base fields - variants are included directly
            if choice_bases.contains(&elem.name) {
                continue;
            }

            // Handle backbone elements - generate inline type
            // Use underscore separator to avoid collision with mutation input types
            // (e.g., Task_Input for backbone vs TaskInput for mutation input)
            if elem.element_type == "BackboneElement" {
                let backbone_type_name = format!("{}_{}", type_name, capitalize_first(&elem.name));
                self.generate_backbone_type(type_name, &elem.name, &backbone_type_name)
                    .await?;

                // BackboneElements are usually arrays
                let type_ref = TypeRef::named_list(&backbone_type_name);
                let field = create_field_resolver(&elem.name, type_ref);
                obj = obj.field(field);
                has_fields = true;
            } else {
                // Queue referenced types for generation
                self.queue_referenced_type(&elem.element_type);

                // Determine if this is likely an array based on common patterns
                let is_array = is_likely_array(&elem.name, &elem.element_type);

                // Get the GraphQL type reference
                let type_ref = self.get_type_ref(&elem.element_type, is_array);
                let field = create_field_resolver(&elem.name, type_ref);
                obj = obj.field(field);
                has_fields = true;
            }
        }

        // GraphQL requires at least one field per type. If the model provider
        // returned no elements (e.g., for testing with EmptyModelProvider),
        // add a placeholder field to satisfy the GraphQL schema requirement.
        if !has_fields {
            trace!(type_name = %type_name, "No elements found, adding placeholder field");
            let placeholder = Field::new("_placeholder", TypeRef::named(TypeRef::STRING), |_ctx| {
                FieldFuture::new(async { Ok(None::<Value>) })
            })
            .description("Placeholder field - type has no defined elements");
            obj = obj.field(placeholder);
        }

        self.registry.finish_generating(type_name, obj);
        Ok(())
    }

    /// Generates an inline BackboneElement type.
    ///
    /// This method looks up the parent type by name, so it can only be called
    /// for top-level backbone elements (where parent_type is a real FHIR type).
    /// For nested backbones, use `generate_backbone_type_from_info`.
    async fn generate_backbone_type(
        &mut self,
        parent_type: &str,
        element_name: &str,
        type_name: &str,
    ) -> Result<(), GraphQLError> {
        if self.registry.is_known(type_name) {
            return Ok(());
        }

        debug!(
            parent = %parent_type,
            element = %element_name,
            type_name = %type_name,
            "Generating backbone type"
        );

        // Get the TypeInfo for the parent type
        let parent_type_info = self
            .model_provider
            .get_type(parent_type)
            .await
            .map_err(|e| {
                GraphQLError::SchemaBuildFailed(format!(
                    "Failed to get type {}: {}",
                    parent_type, e
                ))
            })?;

        // Get element type for the backbone
        let backbone_type = if let Some(parent_info) = &parent_type_info {
            match self
                .model_provider
                .get_element_type(parent_info, element_name)
                .await
            {
                Ok(Some(info)) => {
                    debug!(
                        type_name = %type_name,
                        backbone_info_name = ?info.name,
                        "Got backbone element TypeInfo"
                    );
                    Some(info)
                }
                Ok(None) => {
                    debug!(
                        parent = %parent_type,
                        element = %element_name,
                        "No TypeInfo found for backbone element"
                    );
                    None
                }
                Err(e) => {
                    debug!(
                        parent = %parent_type,
                        element = %element_name,
                        error = %e,
                        "Error getting backbone element TypeInfo"
                    );
                    None
                }
            }
        } else {
            debug!(
                parent = %parent_type,
                "Parent type not found in model provider"
            );
            None
        };

        // Generate the type using the backbone TypeInfo
        self.generate_backbone_type_from_info(type_name, backbone_type.as_ref())
            .await
    }

    /// Generates a backbone element type from its TypeInfo.
    ///
    /// This is the core backbone generation logic that works with the TypeInfo directly,
    /// allowing it to recursively generate nested backbones.
    async fn generate_backbone_type_from_info(
        &mut self,
        type_name: &str,
        backbone_type: Option<&octofhir_fhir_model::provider::TypeInfo>,
    ) -> Result<(), GraphQLError> {
        if self.registry.is_known(type_name) {
            debug!(type_name = %type_name, "Backbone type already known, skipping");
            return Ok(());
        }

        debug!(
            type_name = %type_name,
            has_info = backbone_type.is_some(),
            "Starting backbone type generation"
        );
        self.registry.start_generating(type_name);

        let mut obj =
            Object::new(type_name).description(format!("FHIR backbone element: {}", type_name));
        let mut has_fields = false;

        if let Some(backbone_info) = backbone_type {
            // Get element names for the backbone
            let child_names = self.model_provider.get_element_names(backbone_info);

            for child_name in child_names {
                if let Ok(Some(child_type)) = self
                    .model_provider
                    .get_element_type(backbone_info, &child_name)
                    .await
                    && let Some(element_type) = &child_type.name
                {
                    // Handle nested BackboneElements recursively.
                    // Backbone elements are identified by having a path-based name (e.g., "Task.input")
                    // instead of a simple type name like "string" or "CodeableConcept".
                    let is_nested_backbone = element_type.contains('.');
                    if is_nested_backbone {
                        // Use underscore separator to avoid collision with mutation input types
                        let nested_type_name =
                            format!("{}_{}", type_name, capitalize_first(&child_name));

                        // child_type IS the TypeInfo for this nested backbone element
                        // Recursively generate the nested backbone type with its TypeInfo
                        Box::pin(self.generate_backbone_type_from_info(
                            &nested_type_name,
                            Some(&child_type),
                        ))
                        .await?;

                        // Create the field reference
                        let is_array = is_likely_array(&child_name, element_type);
                        let type_ref = if is_array {
                            TypeRef::named_list(&nested_type_name)
                        } else {
                            TypeRef::named(&nested_type_name)
                        };
                        let field = create_field_resolver(&child_name, type_ref);
                        obj = obj.field(field);
                        has_fields = true;
                    } else {
                        self.queue_referenced_type(element_type);

                        let is_array = is_likely_array(&child_name, element_type);
                        let type_ref = self.get_type_ref(element_type, is_array);
                        let field = create_field_resolver(&child_name, type_ref);
                        obj = obj.field(field);
                        has_fields = true;
                    }
                }
            }
        }

        // GraphQL requires at least one field per type
        if !has_fields {
            trace!(type_name = %type_name, "No elements found for backbone, adding placeholder field");
            let placeholder = Field::new("_placeholder", TypeRef::named(TypeRef::STRING), |_ctx| {
                FieldFuture::new(async { Ok(None::<Value>) })
            })
            .description("Placeholder field - backbone has no defined elements");
            obj = obj.field(placeholder);
        }

        self.registry.finish_generating(type_name, obj);
        Ok(())
    }

    /// Gets choice types for an element if it's a choice base.
    async fn get_choice_types(&self, type_name: &str, element_name: &str) -> Option<Vec<String>> {
        if let Ok(Some(choices)) = self
            .model_provider
            .get_choice_types(type_name, element_name)
            .await
        {
            let type_names: Vec<String> = choices.iter().map(|c| c.type_name.clone()).collect();
            if !type_names.is_empty() {
                return Some(type_names);
            }
        }
        None
    }

    /// Creates a GraphQL TypeRef with array handling.
    fn get_type_ref(&self, fhir_type: &str, is_array: bool) -> TypeRef {
        let base_type = fhir_type_to_graphql(fhir_type);

        if is_array {
            // [Type] - nullable array with nullable elements
            TypeRef::named_list(get_type_name(&base_type))
        } else {
            // Type - nullable single value
            base_type
        }
    }

    /// Queues a type for generation if it needs a GraphQL Object type.
    fn queue_referenced_type(&mut self, type_name: &str) {
        // Skip primitives - they use scalars
        if is_fhir_primitive(type_name) {
            return;
        }
        // Skip types we already know about
        if self.registry.is_known(type_name) {
            return;
        }
        self.registry.queue_if_needed(type_name);
    }
}

/// Creates a field resolver that extracts a value from the parent JSON object.
///
/// Sanitizes field names for GraphQL compatibility by replacing hyphens with underscores.
/// GraphQL field names must match [_a-zA-Z0-9], but FHIR element names can contain hyphens.
fn create_field_resolver(field_name: &str, type_ref: TypeRef) -> Field {
    // Sanitize field name for GraphQL - replace hyphens with underscores
    let graphql_field_name = field_name.replace('-', "_");

    // Keep original field name for JSON lookup (FHIR resources use the original name)
    let json_field_name = field_name.to_string();

    Field::new(&graphql_field_name, type_ref, move |ctx| {
        let field_name = json_field_name.clone();
        FieldFuture::new(async move {
            if let Some(parent) = ctx.parent_value.as_value()
                && let Value::Object(obj) = parent
                && let Some(value) = obj.get(&async_graphql::Name::new(&field_name))
            {
                return Ok(Some(value.clone()));
            }
            Ok(None)
        })
    })
}

/// Checks if a type is a FHIR primitive type (uses scalar).
fn is_fhir_primitive(type_name: &str) -> bool {
    matches!(
        type_name,
        "boolean"
            | "integer"
            | "integer64"
            | "string"
            | "decimal"
            | "uri"
            | "url"
            | "canonical"
            | "base64Binary"
            | "instant"
            | "date"
            | "dateTime"
            | "time"
            | "code"
            | "oid"
            | "uuid"
            | "id"
            | "markdown"
            | "unsignedInt"
            | "positiveInt"
            | "xhtml"
    )
}

/// Heuristic to determine if a field is likely an array.
///
/// Since ElementInfo doesn't include cardinality, we use naming patterns
/// and common FHIR conventions.
fn is_likely_array(field_name: &str, element_type: &str) -> bool {
    // Known array patterns in FHIR
    // Note: managingOrganization is 0..1 in Patient, not an array
    let array_field_names = [
        "identifier",
        "name",
        "telecom",
        "address",
        "contact",
        "communication",
        "link",
        "photo",
        "qualification",
        "generalPractitioner",
        "contained",
        "extension",
        "modifierExtension",
        "coding",
        "line",
        "given",
        "prefix",
        "suffix",
        "performer",
        "basedOn",
        "partOf",
        "category",
        "note",
        "component",
        "referenceRange",
        "hasMember",
        "derivedFrom",
        "interpretation",
        "reaction",
        "entry",
        "issue",
    ];

    // Check if field name suggests array
    if array_field_names.contains(&field_name) {
        return true;
    }

    // Fields ending in common plurals
    if field_name.ends_with("s") && !field_name.ends_with("ss") && !field_name.ends_with("us") {
        // Could be plural
        return true;
    }

    // Extension is always an array
    if element_type == "Extension" {
        return true;
    }

    false
}

/// Capitalizes the first character of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Extracts the type name string from a TypeRef.
fn get_type_name(type_ref: &TypeRef) -> String {
    // TypeRef in async-graphql 7.x uses tuple variants
    match type_ref {
        TypeRef::Named(name) => name.to_string(),
        TypeRef::NonNull(inner) => get_type_name(inner),
        TypeRef::List(inner) => get_type_name(inner),
    }
}

/// Checks if a type name is valid for GraphQL.
///
/// GraphQL names must match the pattern `[_a-zA-Z][_a-zA-Z0-9]*`:
/// - Must start with an underscore or letter (not a number)
/// - Can only contain underscores, letters, and numbers
///
/// This filters out FHIR profiles with invalid characters like hyphens
/// (e.g., "AD-use", "us-core-patient").
fn is_valid_graphql_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();

    // First character must be underscore or letter
    if let Some(first) = chars.next() {
        if !first.is_ascii_alphabetic() && first != '_' {
            return false;
        }
    } else {
        return false;
    }

    // Remaining characters must be underscore, letter, or digit
    for ch in chars {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_registry_queue() {
        let mut registry = TypeRegistry::new();

        registry.queue_if_needed("Patient");
        registry.queue_if_needed("HumanName");
        registry.queue_if_needed("Patient"); // Duplicate

        assert_eq!(registry.pop_pending(), Some("Patient".to_string()));
        assert_eq!(registry.pop_pending(), Some("HumanName".to_string()));
        assert_eq!(registry.pop_pending(), None);
    }

    #[test]
    fn test_type_registry_cycle_detection() {
        let mut registry = TypeRegistry::new();

        registry.start_generating("Patient");
        assert!(registry.is_known("Patient"));

        // Should not be queued while generating
        registry.queue_if_needed("Patient");
        assert_eq!(registry.pop_pending(), None);
    }

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("name"), "Name");
        assert_eq!(capitalize_first("birthDate"), "BirthDate");
        assert_eq!(capitalize_first(""), "");
        assert_eq!(capitalize_first("a"), "A");
    }

    #[test]
    fn test_is_fhir_primitive() {
        assert!(is_fhir_primitive("string"));
        assert!(is_fhir_primitive("boolean"));
        assert!(is_fhir_primitive("dateTime"));
        assert!(!is_fhir_primitive("Patient"));
        assert!(!is_fhir_primitive("HumanName"));
    }

    #[test]
    fn test_is_likely_array() {
        assert!(is_likely_array("identifier", "Identifier"));
        assert!(is_likely_array("name", "HumanName"));
        assert!(is_likely_array("extension", "Extension"));
        assert!(!is_likely_array("birthDate", "date"));
        assert!(!is_likely_array("gender", "code"));
    }

    #[test]
    fn test_is_valid_graphql_name() {
        // Valid names
        assert!(is_valid_graphql_name("Patient"));
        assert!(is_valid_graphql_name("HumanName"));
        assert!(is_valid_graphql_name("_internalType"));
        assert!(is_valid_graphql_name("Type123"));
        assert!(is_valid_graphql_name("Some_Type_Name"));

        // Invalid names - contain hyphens (profiles)
        assert!(!is_valid_graphql_name("AD-use"));
        assert!(!is_valid_graphql_name("us-core-patient"));
        assert!(!is_valid_graphql_name("some-profile"));

        // Invalid names - start with number
        assert!(!is_valid_graphql_name("123Type"));

        // Invalid names - empty or special characters
        assert!(!is_valid_graphql_name(""));
        assert!(!is_valid_graphql_name("Type.Name"));
        assert!(!is_valid_graphql_name("Type Name"));
        assert!(!is_valid_graphql_name("Type@Name"));
    }
}
