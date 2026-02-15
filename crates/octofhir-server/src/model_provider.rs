//! Server-wide model provider with database-backed on-demand schema loading.
//!
//! This implementation loads FHIR schemas from the database on-demand using an LRU cache,
//! rather than loading all schemas into memory at startup. This reduces memory usage
//! and startup time for servers with large numbers of profiles.

use std::sync::Arc;

use async_trait::async_trait;
use moka::future::Cache;
use octofhir_fhir_model::error::Result as ModelResult;
use octofhir_fhir_model::provider::{
    ChoiceTypeInfo, ElementInfo, FhirVersion, ModelProvider, TypeInfo,
};
use octofhir_fhirschema::SchemaProvider;
use octofhir_fhirschema::types::FhirSchema;
use octofhir_search::ElementTypeResolver;
use sqlx_postgres::PgPool;
use tracing::{debug, warn};

use octofhir_db_postgres::PostgresPackageStore;

/// FHIR to FHIRPath type mapping - essential for type conversion
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

/// Server-wide model provider with database-backed on-demand schema loading.
///
/// Schemas are loaded from the `fcm.fhirschemas` table on-demand and cached
/// using an LRU cache (moka). This reduces memory usage compared to loading
/// all schemas at startup.
#[derive(Debug)]
pub struct OctoFhirModelProvider {
    /// Database connection pool
    pool: PgPool,
    /// LRU cache for schemas by name (e.g., "Patient", "Observation")
    cache: Cache<String, Option<Arc<FhirSchema>>>,
    /// LRU cache for schemas by canonical URL (for meta.profile validation)
    url_cache: Cache<String, Option<Arc<FhirSchema>>>,
    /// FHIR version this provider serves
    fhir_version: FhirVersion,
    /// FHIR version string for database queries (e.g., "4.0.1", "4.3.0")
    fhir_version_str: String,
    /// Type mapping for FHIR to FHIRPath conversion
    type_mapping: std::collections::HashMap<String, String>,
    /// Reverse mapping for FHIRPath to FHIR types
    reverse_type_mapping: std::collections::HashMap<String, String>,
}

impl OctoFhirModelProvider {
    /// Create a new model provider with database-backed schema loading.
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    /// * `fhir_version` - FHIR version to serve
    /// * `cache_size` - Maximum number of schemas to keep in cache
    pub fn new(pool: PgPool, fhir_version: FhirVersion, cache_size: u64) -> Self {
        let type_mapping: std::collections::HashMap<String, String> = TYPE_MAPPING
            .iter()
            .map(|(fhir, fhirpath)| (fhir.to_string(), fhirpath.to_string()))
            .collect();

        let reverse_type_mapping: std::collections::HashMap<String, String> = TYPE_MAPPING
            .iter()
            .map(|(fhir, fhirpath)| (fhirpath.to_string(), fhir.to_string()))
            .collect();

        // Use short version names to match what's stored in the database
        // The canonical manager stores with cfg.fhir.version (e.g., "R4", "R4B")
        let fhir_version_str = match &fhir_version {
            FhirVersion::R4 => "R4".to_string(),
            FhirVersion::R4B => "R4B".to_string(),
            FhirVersion::R5 => "R5".to_string(),
            FhirVersion::R6 => "R6".to_string(),
            FhirVersion::Custom { version } => version.clone(),
        };

        Self {
            pool,
            cache: Cache::new(cache_size),
            url_cache: Cache::new(cache_size),
            fhir_version,
            fhir_version_str,
            type_mapping,
            reverse_type_mapping,
        }
    }

    /// Get access to the database pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get the FHIR version string for database queries
    pub fn fhir_version_str(&self) -> &str {
        &self.fhir_version_str
    }

    /// Clear cached schemas to allow newly installed packages to be visible.
    pub fn invalidate_schema_caches(&self) {
        self.cache.invalidate_all();
        self.url_cache.invalidate_all();
    }

    /// Get a schema by name (e.g., "Patient", "Observation").
    ///
    /// Checks the cache first, then loads from database if not found.
    pub async fn get_schema(&self, name: &str) -> Option<Arc<FhirSchema>> {
        // Check cache first
        if let Some(schema) = self.cache.get(name).await.flatten() {
            return Some(schema);
        }

        let name_key = name.to_string();
        let fhir_version_str = self.fhir_version_str.clone();
        let pool = self.pool.clone();
        let url_cache = self.url_cache.clone();

        self.cache
            .get_with(name_key.clone(), async move {
                let store = PostgresPackageStore::new(pool);
                match store
                    .get_fhirschema_by_name(&name_key, &fhir_version_str)
                    .await
                {
                    Ok(Some(record)) => {
                        match serde_json::from_value::<FhirSchema>(record.content) {
                            Ok(schema) => {
                                let schema = Arc::new(schema);
                                url_cache
                                    .insert(schema.url.clone(), Some(schema.clone()))
                                    .await;
                                Some(schema)
                            }
                            Err(e) => {
                                warn!("Failed to deserialize FhirSchema for {}: {}", name_key, e);
                                None
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("Schema not found in database: {}", name_key);
                        None
                    }
                    Err(e) => {
                        warn!("Database error loading schema {}: {}", name_key, e);
                        None
                    }
                }
            })
            .await
    }

    /// Get a schema by canonical URL (for meta.profile validation).
    ///
    /// e.g., "http://hl7.org/fhir/StructureDefinition/Patient"
    pub async fn get_schema_by_url(&self, url: &str) -> Option<Arc<FhirSchema>> {
        // Check URL cache first
        if let Some(schema) = self.url_cache.get(url).await.flatten() {
            return Some(schema);
        }

        let url_key = url.to_string();
        let fhir_version_str = self.fhir_version_str.clone();
        let pool = self.pool.clone();
        let name_cache = self.cache.clone();

        self.url_cache
            .get_with(url_key.clone(), async move {
                let store = PostgresPackageStore::new(pool);
                match store
                    .get_fhirschema_by_url(&url_key, &fhir_version_str)
                    .await
                {
                    Ok(Some(record)) => {
                        match serde_json::from_value::<FhirSchema>(record.content) {
                            Ok(schema) => {
                                let schema = Arc::new(schema);
                                name_cache
                                    .insert(schema.name.clone(), Some(schema.clone()))
                                    .await;
                                Some(schema)
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to deserialize FhirSchema for URL {}: {}",
                                    url_key, e
                                );
                                None
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("Schema not found by URL: {}", url_key);
                        None
                    }
                    Err(e) => {
                        warn!("Database error loading schema by URL {}: {}", url_key, e);
                        None
                    }
                }
            })
            .await
    }

    /// Check if an element is a choice type variant and return the base element name.
    ///
    /// E.g., for Patient type and "deceasedBoolean" element, returns Some("deceased").
    /// This is now async since the schema must be loaded on demand.
    pub async fn get_choice_base_name(
        &self,
        type_name: &str,
        element_name: &str,
    ) -> Option<String> {
        let schema = self.get_schema(type_name).await?;
        let elements = schema.elements.as_ref()?;

        // Look up the element in the schema
        if let Some(element) = elements.get(element_name) {
            // If this element has a choice_of field, it's a choice variant
            if let Some(base_name) = &element.choice_of {
                return Some(base_name.clone());
            }
        }

        None
    }

    /// Map FHIR type to FHIRPath type
    fn map_fhir_type(&self, fhir_type: &str) -> String {
        self.type_mapping
            .get(fhir_type)
            .cloned()
            .unwrap_or_else(|| fhir_type.to_string())
    }

    /// Get backbone element's nested elements by parent type and path.
    ///
    /// Returns a reference to the nested elements HashMap without cloning during navigation.
    /// Only clones at the end when returning the result.
    async fn get_backbone_elements_by_path(
        &self,
        parent_type: &str,
        element_path: &str,
    ) -> Option<std::collections::HashMap<String, octofhir_fhirschema::types::FhirSchemaElement>>
    {
        let schema = self.get_schema(parent_type).await?;
        let mut current_elements = schema.elements.as_ref()?;

        // Navigate through the path using references (no cloning during navigation)
        for part in element_path.split('.') {
            let element = current_elements.get(part)?;
            current_elements = element.elements.as_ref()?;
        }

        // Only clone at the end when we need to return
        Some(current_elements.clone())
    }
}

#[async_trait]
impl ModelProvider for OctoFhirModelProvider {
    async fn get_type(&self, type_name: &str) -> ModelResult<Option<TypeInfo>> {
        if let Some(schema) = self.get_schema(type_name).await {
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
        } else {
            // Check if it's a primitive type in our mapping
            if let Some(mapped) = self.type_mapping.get(type_name) {
                Ok(Some(TypeInfo {
                    type_name: mapped.clone(),
                    singleton: Some(true),
                    is_empty: Some(false),
                    namespace: Some("System".to_string()),
                    name: Some(type_name.to_string()),
                }))
            } else if self.reverse_type_mapping.contains_key(type_name) {
                Ok(Some(TypeInfo {
                    type_name: type_name.to_string(),
                    singleton: Some(true),
                    is_empty: Some(false),
                    namespace: Some("System".to_string()),
                    name: Some(type_name.to_string()),
                }))
            } else {
                Ok(None)
            }
        }
    }

    async fn get_element_type(
        &self,
        parent_type: &TypeInfo,
        property_name: &str,
    ) -> ModelResult<Option<TypeInfo>> {
        if let Some(type_name) = &parent_type.name {
            // Check if this is a backbone element path
            let elements = if type_name.contains('.') {
                let parts: Vec<&str> = type_name.splitn(2, '.').collect();
                if parts.len() == 2 {
                    self.get_backbone_elements_by_path(parts[0], parts[1]).await
                } else {
                    None
                }
            } else {
                self.get_schema(type_name)
                    .await
                    .and_then(|schema| schema.elements.clone())
            };

            let Some(elements) = elements else {
                return Ok(None);
            };

            // Try direct property name match
            if let Some(element) = elements.get(property_name) {
                // Check if this is a backbone element
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

                // Regular element with type_name
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

            // Handle choice navigation (e.g., value[x] -> valueString)
            for (element_name, element) in &elements {
                if element_name.ends_with("[x]") {
                    let base_name = element_name.trim_end_matches("[x]");
                    if let Some(type_suffix) = property_name.strip_prefix(base_name)
                        && !type_suffix.is_empty()
                    {
                        let mut chars = type_suffix.chars();
                        if let Some(first_char) = chars.next() {
                            let schema_type =
                                format!("{}{}", first_char.to_lowercase(), chars.as_str());

                            if let Some(choices) = &element.choices
                                && choices.contains(&schema_type)
                            {
                                let mapped_type = self.map_fhir_type(&schema_type);
                                return Ok(Some(TypeInfo {
                                    type_name: mapped_type,
                                    singleton: Some(element.max == Some(1)),
                                    is_empty: Some(false),
                                    namespace: if schema_type.chars().next().unwrap().is_uppercase()
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
        // Direct type match
        if type_info.type_name == target_type {
            return Some(type_info.clone());
        }

        // Name match
        if let Some(ref name) = type_info.name
            && name == target_type
        {
            return Some(type_info.clone());
        }

        // Note: is_type_derived_from is sync, but we can't make it async here
        // For now, rely on direct matches. Full hierarchy check would need
        // to be done separately.
        None
    }

    fn get_element_names(&self, parent_type: &TypeInfo) -> Vec<String> {
        // This is a sync method but schema loading is async
        // We can't do async lookup here, so return empty
        // Callers should use get_elements() which is async for complete element info
        let _ = parent_type;
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

        // Collect elements from the type hierarchy
        let mut current_type = Some(type_name.to_string());
        while let Some(ref type_to_check) = current_type {
            if let Some(schema) = self.get_schema(type_to_check).await {
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

                // Move to parent type
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
        let store = PostgresPackageStore::new(self.pool.clone());
        // Include both "resource" and "logical" kinds, but exclude profiles
        match store
            .list_fhirschema_names_by_kinds_excluding_profiles(
                &["resource", "logical"],
                &self.fhir_version_str,
            )
            .await
        {
            Ok(names) => Ok(names),
            Err(e) => {
                warn!("Failed to get resource types from database: {}", e);
                Ok(Vec::new())
            }
        }
    }

    async fn get_complex_types(&self) -> ModelResult<Vec<String>> {
        let store = PostgresPackageStore::new(self.pool.clone());
        match store
            .list_fhirschema_names_by_type("complex-type", &self.fhir_version_str)
            .await
        {
            Ok(names) => Ok(names),
            Err(e) => {
                warn!("Failed to get complex types from database: {}", e);
                Ok(Vec::new())
            }
        }
    }

    async fn get_primitive_types(&self) -> ModelResult<Vec<String>> {
        // Primitive types are from our type mapping
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
        Ok(self.get_schema(resource_type).await.is_some())
    }

    async fn get_fhir_version(&self) -> ModelResult<FhirVersion> {
        Ok(self.fhir_version.clone())
    }

    fn is_type_derived_from(&self, derived_type: &str, base_type: &str) -> bool {
        // This is a sync method but schema loading is async
        // Can only do direct equality check here
        // Full hierarchy check requires async check_type_derived()
        derived_type == base_type
    }

    async fn get_choice_types(
        &self,
        parent_type: &str,
        property_name: &str,
    ) -> ModelResult<Option<Vec<ChoiceTypeInfo>>> {
        if let Some(schema) = self.get_schema(parent_type).await
            && let Some(elements) = &schema.elements
        {
            // Look for choice element (property_name[x])
            let choice_key = format!("{}[x]", property_name);
            if let Some(element) = elements.get(&choice_key)
                && let Some(choices) = &element.choices
            {
                let choice_infos: Vec<ChoiceTypeInfo> = choices
                    .iter()
                    .map(|type_name| {
                        // Convert to PascalCase suffix
                        let rest: String = type_name.chars().skip(1).collect();
                        let suffix = type_name
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_default()
                            + rest.as_str();
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
        // Union types not currently supported
        Ok(None)
    }

    fn is_union_type(&self, _type_info: &TypeInfo) -> bool {
        false
    }
}

/// SchemaProvider implementation for lazy schema loading in validation.
///
/// This allows the FhirSchemaValidator to load schemas on-demand from the
/// model provider's Moka LRU cache, avoiding the need to pre-load all schemas.
#[async_trait]
impl SchemaProvider for OctoFhirModelProvider {
    async fn get_schema(&self, name: &str) -> Option<Arc<FhirSchema>> {
        // Delegate to the existing get_schema method
        OctoFhirModelProvider::get_schema(self, name).await
    }

    async fn get_schema_by_url(&self, url: &str) -> Option<Arc<FhirSchema>> {
        // First try by URL (for full canonical URLs like "http://hl7.org/fhir/StructureDefinition/Patient")
        if let Some(schema) = OctoFhirModelProvider::get_schema_by_url(self, url).await {
            return Some(schema);
        }
        // Fall back to name lookup (for short names like "Patient", "AccessPolicy")
        // The SchemaCompiler passes resource type names to get_schema_by_url
        OctoFhirModelProvider::get_schema(self, url).await
    }
}

/// ElementTypeResolver implementation for search parameter type resolution.
///
/// Resolves FHIR element types from FhirSchema at search registry build time,
/// allowing the search engine to generate correct SQL without hardcoded path heuristics.
#[async_trait]
impl ElementTypeResolver for OctoFhirModelProvider {
    async fn resolve(&self, resource_type: &str, element_path: &str) -> Option<(String, bool)> {
        let schema = OctoFhirModelProvider::get_schema(self, resource_type).await?;
        let elements = schema.elements.as_ref()?;

        // Navigate nested paths (e.g., "meta.tag" → meta -> tag)
        let parts: Vec<&str> = element_path.split('.').collect();
        let mut current_elements = elements;

        for (i, part) in parts.iter().enumerate() {
            let element = current_elements.get(*part)?;
            if i == parts.len() - 1 {
                // Found the target element
                return Some((
                    element.type_name.clone().unwrap_or_default(),
                    element.array.unwrap_or(false),
                ));
            }
            // Navigate into nested elements (backbone elements)
            current_elements = element.elements.as_ref()?;
        }

        None
    }
}
