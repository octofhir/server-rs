//! FHIR GraphQL schema builder.
//!
//! This module provides `FhirSchemaBuilder`, which generates a GraphQL schema
//! from FHIR model definitions. The builder uses async-graphql's dynamic schema
//! API to construct the schema at runtime.

use std::sync::Arc;

use async_graphql::dynamic::{Field, FieldFuture, InputValue, Object, Scalar, Schema, SchemaBuilder, TypeRef};
use async_graphql::Value;
use octofhir_search::{SearchParameterRegistry, SearchParameterType};
use tracing::{debug, trace};

use crate::error::GraphQLError;
use crate::resolvers::{ConnectionResolver, ReadResolver, SearchResolver};

/// The name of the custom JSON scalar type used for FHIR resources.
pub const FHIR_RESOURCE_SCALAR: &str = "FhirResource";

/// Configuration for the schema builder.
#[derive(Debug, Clone)]
pub struct SchemaBuilderConfig {
    /// Maximum query depth allowed.
    pub max_depth: usize,

    /// Maximum query complexity allowed.
    pub max_complexity: usize,

    /// Whether to enable introspection queries.
    pub introspection_enabled: bool,
}

impl Default for SchemaBuilderConfig {
    fn default() -> Self {
        Self {
            max_depth: 15,
            max_complexity: 500,
            introspection_enabled: true,
        }
    }
}

/// Builds GraphQL schema from FHIR model definitions.
///
/// `FhirSchemaBuilder` generates a complete GraphQL schema including:
/// - Custom FHIR scalar types
/// - FHIR resource types (Query.Patient, Query.Observation, etc.)
/// - Search parameters as query arguments
/// - Mutation operations (create, update, delete)
///
/// # Example
///
/// ```ignore
/// let builder = FhirSchemaBuilder::new(
///     search_registry,
///     SchemaBuilderConfig::default(),
/// );
///
/// let schema = builder.build().await?;
/// ```
pub struct FhirSchemaBuilder {
    /// Search parameter registry for building query arguments.
    search_registry: Arc<SearchParameterRegistry>,

    /// Configuration options.
    config: SchemaBuilderConfig,
}

impl FhirSchemaBuilder {
    /// Creates a new schema builder.
    #[must_use]
    pub fn new(search_registry: Arc<SearchParameterRegistry>, config: SchemaBuilderConfig) -> Self {
        Self {
            search_registry,
            config,
        }
    }

    /// Builds the GraphQL schema.
    ///
    /// This constructs a complete schema with:
    /// - Custom FHIR scalars
    /// - Query root with resource read and search operations
    /// - Mutation root with create/update/delete operations (stubbed)
    ///
    /// Note: Subscriptions are not included in Phase 1 as they require
    /// special async-graphql handling with proper Subscription objects.
    ///
    /// # Errors
    ///
    /// Returns an error if schema construction fails.
    pub async fn build(&self) -> Result<Schema, GraphQLError> {
        debug!("Starting GraphQL schema build");

        // Create schema builder (no subscription for now - requires special handling)
        let mut schema_builder = Schema::build("Query", Some("Mutation"), None);

        // Register custom scalars
        schema_builder = self.register_scalars(schema_builder);

        // Register connection types for each resource
        schema_builder = self.register_connection_types(schema_builder);

        // Build Query type with resource fields
        let query = self.build_query_type();
        schema_builder = schema_builder.register(query);

        // Build Mutation type (stub)
        let mutation = self.build_mutation_type();
        schema_builder = schema_builder.register(mutation);

        // Configure limits
        let mut schema_builder = schema_builder.limit_depth(self.config.max_depth);
        schema_builder = schema_builder.limit_complexity(self.config.max_complexity);

        // Enable/disable introspection
        if !self.config.introspection_enabled {
            schema_builder = schema_builder.disable_introspection();
        }

        // Build the schema
        let schema = schema_builder
            .finish()
            .map_err(|e| GraphQLError::SchemaBuildFailed(e.to_string()))?;

        debug!("GraphQL schema build complete");
        Ok(schema)
    }

    /// Registers custom FHIR scalar types.
    fn register_scalars(&self, builder: SchemaBuilder) -> SchemaBuilder {
        let scalars = [
            ("FhirInstant", "A FHIR instant (xs:dateTime with timezone)"),
            (
                "FhirDateTime",
                "A FHIR dateTime (partial date/time with optional timezone)",
            ),
            ("FhirDate", "A FHIR date (YYYY, YYYY-MM, or YYYY-MM-DD)"),
            ("FhirTime", "A FHIR time (hh:mm:ss)"),
            ("FhirUri", "A FHIR URI"),
            ("FhirUrl", "A FHIR URL (resolvable URI)"),
            ("FhirCanonical", "A FHIR canonical URL reference"),
            ("FhirOid", "A FHIR OID (urn:oid:...)"),
            ("FhirUuid", "A FHIR UUID (urn:uuid:...)"),
            ("FhirId", "A FHIR resource ID"),
            ("FhirBase64Binary", "Base64-encoded binary data"),
            ("FhirMarkdown", "Markdown-formatted text"),
            ("FhirPositiveInt", "A positive integer (> 0)"),
            ("FhirUnsignedInt", "A non-negative integer (>= 0)"),
            ("FhirDecimal", "An arbitrary precision decimal"),
            ("FhirXhtml", "XHTML content for narratives"),
            // Special scalar for FHIR resources returned as JSON
            (FHIR_RESOURCE_SCALAR, "A FHIR resource represented as JSON"),
        ];

        let mut builder = builder;
        for (name, description) in scalars {
            let scalar = Scalar::new(name).description(description);
            builder = builder.register(scalar);
        }

        builder
    }

    /// Registers connection and edge types for pagination.
    fn register_connection_types(&self, mut builder: SchemaBuilder) -> SchemaBuilder {
        // Get all resource types with search parameters
        let resource_types = self.search_registry.list_resource_types();

        for resource_type in &resource_types {
            // Create Edge type
            let edge_type_name = format!("{}Edge", resource_type);
            let edge = Object::new(&edge_type_name)
                .description(format!("Edge type for {} connection", resource_type))
                .field(
                    Field::new("resource", TypeRef::named_nn(FHIR_RESOURCE_SCALAR), |ctx| {
                        FieldFuture::new(async move {
                            // The resource is stored in parent context
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(resource) = obj.get("resource") {
                                        return Ok(Some(resource.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("The FHIR resource"),
                )
                .field(
                    Field::new("mode", TypeRef::named(TypeRef::STRING), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(mode) = obj.get("mode") {
                                        return Ok(Some(mode.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Search match mode"),
                )
                .field(
                    Field::new("score", TypeRef::named(TypeRef::FLOAT), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(score) = obj.get("score") {
                                        return Ok(Some(score.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Search relevance score"),
                );

            builder = builder.register(edge);

            // Create Connection type
            let connection_type_name = format!("{}Connection", resource_type);
            let edge_type_ref = TypeRef::named_nn_list_nn(&edge_type_name);

            let connection = Object::new(&connection_type_name)
                .description(format!(
                    "Connection type for {} cursor-based pagination",
                    resource_type
                ))
                .field(
                    Field::new("count", TypeRef::named(TypeRef::INT), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(count) = obj.get("count") {
                                        return Ok(Some(count.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Total count of matching resources"),
                )
                .field(
                    Field::new("offset", TypeRef::named(TypeRef::INT), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(offset) = obj.get("offset") {
                                        return Ok(Some(offset.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Current offset into results"),
                )
                .field(
                    Field::new("pageSize", TypeRef::named(TypeRef::INT), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(size) = obj.get("pageSize") {
                                        return Ok(Some(size.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Page size"),
                )
                .field(
                    Field::new("edges", edge_type_ref, |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(edges) = obj.get("edges") {
                                        return Ok(Some(edges.clone()));
                                    }
                                }
                            }
                            Ok(Some(Value::List(vec![])))
                        })
                    })
                    .description("List of edges containing resources"),
                )
                .field(
                    Field::new("first", TypeRef::named(TypeRef::STRING), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(cursor) = obj.get("first") {
                                        return Ok(Some(cursor.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Cursor for first page"),
                )
                .field(
                    Field::new("previous", TypeRef::named(TypeRef::STRING), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(cursor) = obj.get("previous") {
                                        return Ok(Some(cursor.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Cursor for previous page"),
                )
                .field(
                    Field::new("next", TypeRef::named(TypeRef::STRING), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(cursor) = obj.get("next") {
                                        return Ok(Some(cursor.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Cursor for next page"),
                )
                .field(
                    Field::new("last", TypeRef::named(TypeRef::STRING), |ctx| {
                        FieldFuture::new(async move {
                            if let Some(parent) = ctx.parent_value.as_value() {
                                if let Value::Object(obj) = parent {
                                    if let Some(cursor) = obj.get("last") {
                                        return Ok(Some(cursor.clone()));
                                    }
                                }
                            }
                            Ok(None)
                        })
                    })
                    .description("Cursor for last page"),
                );

            builder = builder.register(connection);
        }

        builder
    }

    /// Builds the Query root type with all resource query fields.
    fn build_query_type(&self) -> Object {
        let mut query = Object::new("Query").description("FHIR GraphQL Query root");

        // Add health check field
        query = query.field(
            Field::new("_health", TypeRef::named_nn(TypeRef::STRING), |_| {
                FieldFuture::new(async { Ok(Some(Value::String("ok".to_string()))) })
            })
            .description("Health check endpoint"),
        );

        // Add version field
        query = query.field(
            Field::new("_version", TypeRef::named_nn(TypeRef::STRING), |_| {
                FieldFuture::new(async {
                    Ok(Some(Value::String(
                        env!("CARGO_PKG_VERSION").to_string(),
                    )))
                })
            })
            .description("API version"),
        );

        // Get resource types from search registry
        let resource_types = self.search_registry.list_resource_types();
        debug!(
            resource_count = resource_types.len(),
            "Building Query type for FHIR resources"
        );

        // For each resource type, add:
        // - ResourceType(_id: ID!): FhirResource - single resource read
        // - ResourceTypeList(...): [FhirResource!]! - list/search query
        // - ResourceTypeConnection(...): ResourceTypeConnection! - cursor pagination
        for resource_type in resource_types {
            query = self.add_resource_query_fields(query, resource_type);
        }

        query
    }

    /// Adds query fields for a specific resource type.
    fn add_resource_query_fields(&self, mut query: Object, resource_type: &str) -> Object {
        let resource_type_owned = resource_type.to_string();

        // 1. Single resource read: Patient(_id: ID!): FhirResource
        let read_field_name = resource_type;
        let read_resolver = ReadResolver::resolve(resource_type_owned.clone());

        let read_field = Field::new(read_field_name, TypeRef::named(FHIR_RESOURCE_SCALAR), read_resolver)
            .argument(InputValue::new("_id", TypeRef::named_nn(TypeRef::ID)))
            .description(format!("Read a single {} resource by ID", resource_type));

        query = query.field(read_field);
        trace!(resource_type = %resource_type, "Added read query field");

        // 2. List query: PatientList(...): [FhirResource!]!
        let list_field_name = format!("{}List", resource_type);
        let search_resolver = SearchResolver::resolve(resource_type_owned.clone());

        let mut list_field =
            Field::new(&list_field_name, TypeRef::named_nn_list_nn(FHIR_RESOURCE_SCALAR), search_resolver)
                .description(format!("Search for {} resources", resource_type));

        // Add search parameter arguments
        list_field = self.add_search_arguments(list_field, resource_type);

        query = query.field(list_field);
        trace!(resource_type = %resource_type, "Added list query field");

        // 3. Connection query: PatientConnection(...): PatientConnection!
        let connection_type_name = format!("{}Connection", resource_type);
        let connection_field_name = connection_type_name.clone();
        let connection_resolver = ConnectionResolver::resolve(resource_type_owned);

        let mut connection_field = Field::new(
            &connection_field_name,
            TypeRef::named_nn(&connection_type_name),
            connection_resolver,
        )
        .description(format!(
            "Search for {} resources with cursor-based pagination",
            resource_type
        ));

        // Add search parameter arguments plus cursor
        connection_field = self.add_search_arguments(connection_field, resource_type);
        connection_field = connection_field.argument(InputValue::new(
            "cursor",
            TypeRef::named(TypeRef::STRING),
        ));

        query = query.field(connection_field);
        trace!(resource_type = %resource_type, "Added connection query field");

        query
    }

    /// Adds search parameter arguments to a field.
    fn add_search_arguments(&self, mut field: Field, resource_type: &str) -> Field {
        // Get all search parameters for this resource type
        let params = self.search_registry.get_all_for_type(resource_type);

        for param in params {
            // Convert FHIR param code to GraphQL-safe name
            // GraphQL doesn't allow hyphens in field names
            let graphql_name = param.code.replace('-', "_");

            // Determine GraphQL type based on FHIR parameter type
            let type_ref = match param.param_type {
                SearchParameterType::Number => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Date => TypeRef::named(TypeRef::STRING),
                SearchParameterType::String => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Token => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Reference => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Composite => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Quantity => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Uri => TypeRef::named(TypeRef::STRING),
                SearchParameterType::Special => TypeRef::named(TypeRef::STRING),
            };

            let input = InputValue::new(&graphql_name, type_ref);
            field = field.argument(input);
        }

        // Add common pagination/control arguments
        field = field.argument(InputValue::new("_count", TypeRef::named(TypeRef::INT)));
        field = field.argument(InputValue::new("_offset", TypeRef::named(TypeRef::INT)));
        field = field.argument(InputValue::new("_sort", TypeRef::named(TypeRef::STRING)));
        field = field.argument(InputValue::new("_filter", TypeRef::named(TypeRef::STRING)));

        // Add _reference for reverse reference queries (FHIR GraphQL spec)
        // This specifies which reference search parameter to use when finding
        // resources that reference the focused resource
        field = field.argument(InputValue::new("_reference", TypeRef::named(TypeRef::STRING)));

        field
    }

    /// Builds the Mutation root type (stub).
    fn build_mutation_type(&self) -> Object {
        let mut mutation = Object::new("Mutation").description("FHIR GraphQL Mutation root");

        // Placeholder field (required for valid schema)
        mutation = mutation.field(
            Field::new("_placeholder", TypeRef::named(TypeRef::STRING), |_| {
                FieldFuture::new(async { Ok(None::<Value>) })
            })
            .description("Placeholder - mutations will be added in Phase 4"),
        );

        mutation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SchemaBuilderConfig::default();
        assert_eq!(config.max_depth, 15);
        assert_eq!(config.max_complexity, 500);
        assert!(config.introspection_enabled);
    }

    #[tokio::test]
    async fn test_schema_builder_creates_valid_schema() {
        // Create a minimal search registry
        let registry = Arc::new(SearchParameterRegistry::default());

        let builder = FhirSchemaBuilder::new(registry, SchemaBuilderConfig::default());

        let result = builder.build().await;
        assert!(result.is_ok(), "Schema should build successfully");

        let schema = result.unwrap();

        // Verify schema has Query type
        let sdl = schema.sdl();
        assert!(sdl.contains("type Query"), "Schema should have Query type");
        assert!(
            sdl.contains("type Mutation"),
            "Schema should have Mutation type"
        );

        // Verify custom scalars are registered
        assert!(
            sdl.contains("scalar FhirInstant"),
            "Schema should have FhirInstant scalar"
        );
        assert!(
            sdl.contains("scalar FhirDateTime"),
            "Schema should have FhirDateTime scalar"
        );
        assert!(
            sdl.contains("scalar FhirId"),
            "Schema should have FhirId scalar"
        );
    }

    #[tokio::test]
    async fn test_schema_has_health_field() {
        let registry = Arc::new(SearchParameterRegistry::default());
        let builder = FhirSchemaBuilder::new(registry, SchemaBuilderConfig::default());

        let schema = builder.build().await.unwrap();
        let sdl = schema.sdl();

        assert!(
            sdl.contains("_health"),
            "Schema should have _health field on Query"
        );
        assert!(
            sdl.contains("_version"),
            "Schema should have _version field on Query"
        );
    }

    #[tokio::test]
    async fn test_schema_with_resource_types() {
        use octofhir_search::SearchParameter;

        // Create a search registry with Patient parameters
        let mut registry = SearchParameterRegistry::new();
        registry.register(SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        ));
        registry.register(SearchParameter::new(
            "birthdate",
            "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
            SearchParameterType::Date,
            vec!["Patient".to_string()],
        ));

        let builder = FhirSchemaBuilder::new(Arc::new(registry), SchemaBuilderConfig::default());

        let schema = builder.build().await.unwrap();
        let sdl = schema.sdl();

        // Should have Patient query field
        assert!(
            sdl.contains("Patient("),
            "Schema should have Patient read query"
        );

        // Should have PatientList query field
        assert!(
            sdl.contains("PatientList("),
            "Schema should have PatientList query"
        );

        // Should have PatientConnection query field
        assert!(
            sdl.contains("PatientConnection("),
            "Schema should have PatientConnection query"
        );

        // Should have connection type
        assert!(
            sdl.contains("type PatientConnection"),
            "Schema should have PatientConnection type"
        );

        // Should have edge type
        assert!(
            sdl.contains("type PatientEdge"),
            "Schema should have PatientEdge type"
        );
    }

    #[tokio::test]
    async fn test_schema_with_disabled_introspection() {
        let registry = Arc::new(SearchParameterRegistry::default());
        let config = SchemaBuilderConfig {
            introspection_enabled: false,
            ..Default::default()
        };

        let builder = FhirSchemaBuilder::new(registry, config);
        let result = builder.build().await;

        assert!(
            result.is_ok(),
            "Schema should build with introspection disabled"
        );
    }
}
