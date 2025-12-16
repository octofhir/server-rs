//! FHIR GraphQL schema builder.
//!
//! This module provides `FhirSchemaBuilder`, which generates a GraphQL schema
//! from FHIR model definitions. The builder uses async-graphql's dynamic schema
//! API to construct the schema at runtime.

use std::sync::Arc;

use async_graphql::Value;
use async_graphql::dynamic::{
    Field, FieldFuture, InputValue, Object, Scalar, Schema, SchemaBuilder, TypeRef,
};
use octofhir_fhir_model::provider::ModelProvider;
use octofhir_search::{SearchParameterRegistry, SearchParameterType};
use tracing::{debug, trace};

use super::directives::log_fhir_directives_support;
use super::input_types::{
    InputTypeGenerator, create_json_scalar, create_operation_outcome_issue_type,
};
use super::type_generator::FhirTypeGenerator;
use crate::subscriptions::{ResourceEventBroadcaster, build_subscription_type, create_resource_change_event_type};
use crate::error::GraphQLError;
use crate::resolvers::{
    ConnectionResolver, CreateResolver, DeleteResolver, NestedReverseReferenceResolver,
    ReadResolver, SearchResolver, UpdateResolver,
};
use crate::types::{create_all_resources_union, create_reference_type};

/// The name of the custom JSON scalar type used for FHIR resources.
/// Kept for backwards compatibility but no longer primary mechanism.
pub const FHIR_RESOURCE_SCALAR: &str = "FhirResource";

/// Internal resource types that should always be included in the GraphQL schema.
/// These are OctoFHIR-specific resources used for authentication, authorization, and gateway functionality.
const INTERNAL_RESOURCE_TYPES: &[&str] = &[
    "User",
    "Client",
    "AccessPolicy",
    "Session",
    "RefreshToken",
    "RevokedToken",
    "IdentityProvider",
    "App",
    "CustomOperation",
];

/// Configuration for the schema builder.
#[derive(Debug, Clone)]
pub struct SchemaBuilderConfig {
    /// Maximum query depth allowed.
    pub max_depth: usize,

    /// Maximum query complexity allowed.
    pub max_complexity: usize,

    /// Whether to enable introspection queries.
    pub introspection_enabled: bool,

    /// Whether to enable subscriptions.
    pub subscriptions_enabled: bool,
}

impl Default for SchemaBuilderConfig {
    fn default() -> Self {
        Self {
            max_depth: 15,
            max_complexity: 500,
            introspection_enabled: true,
            subscriptions_enabled: false,
        }
    }
}

/// Dynamic model provider type alias for convenience.
pub type DynModelProvider = Arc<dyn ModelProvider + Send + Sync>;

/// Builds GraphQL schema from FHIR model definitions.
///
/// `FhirSchemaBuilder` generates a complete GraphQL schema including:
/// - Custom FHIR scalar types
/// - FHIR resource and complex types with typed fields
/// - Search parameters as query arguments
/// - Mutation operations (create, update, delete)
///
/// # Example
///
/// ```ignore
/// // Use existing model provider from server (OctoFhirModelProvider)
/// let builder = FhirSchemaBuilder::new(
///     search_registry,
///     model_provider,  // Arc<dyn ModelProvider>
///     SchemaBuilderConfig::default(),
/// );
///
/// let schema = builder.build().await?;
/// ```
pub struct FhirSchemaBuilder {
    /// Search parameter registry for building query arguments.
    search_registry: Arc<SearchParameterRegistry>,

    /// Model provider for FHIR type introspection (reuses existing provider).
    model_provider: DynModelProvider,

    /// Configuration options.
    config: SchemaBuilderConfig,

    /// Optional event broadcaster for subscriptions.
    event_broadcaster: Option<Arc<ResourceEventBroadcaster>>,
}

impl FhirSchemaBuilder {
    /// Creates a new schema builder.
    ///
    /// The `model_provider` should be the same instance used for validation,
    /// FHIRPath, and LSP (e.g., `OctoFhirModelProvider` from the server).
    #[must_use]
    pub fn new(
        search_registry: Arc<SearchParameterRegistry>,
        model_provider: DynModelProvider,
        config: SchemaBuilderConfig,
    ) -> Self {
        Self {
            search_registry,
            model_provider,
            config,
            event_broadcaster: None,
        }
    }

    /// Sets the event broadcaster for subscriptions.
    ///
    /// When set, the schema will include subscription fields for resource changes.
    #[must_use]
    pub fn with_event_broadcaster(mut self, broadcaster: Arc<ResourceEventBroadcaster>) -> Self {
        self.event_broadcaster = Some(broadcaster);
        self
    }

    /// Builds the GraphQL schema.
    ///
    /// This constructs a complete schema with:
    /// - Custom FHIR scalars
    /// - FHIR resource and complex types with typed fields
    /// - Query root with resource read and search operations
    /// - Mutation root with create/update/delete operations
    /// - Subscription root for real-time resource changes (when enabled)
    ///
    /// # Errors
    ///
    /// Returns an error if schema construction fails.
    pub async fn build(&self) -> Result<Schema, GraphQLError> {
        debug!("Starting GraphQL schema build");

        // Determine if subscriptions should be enabled
        let subscription_name = if self.config.subscriptions_enabled && self.event_broadcaster.is_some() {
            Some("Subscription")
        } else {
            None
        };

        // Create schema builder
        let mut schema_builder = Schema::build("Query", Some("Mutation"), subscription_name);

        // Register custom scalars
        schema_builder = self.register_scalars(schema_builder);

        // Log FHIR GraphQL directives support
        // Note: async-graphql dynamic schema doesn't support custom directive registration,
        // so directives are handled via response transformation in the handler
        log_fhir_directives_support();

        // Generate and register FHIR types using the type generator
        let mut type_generator = FhirTypeGenerator::new(self.model_provider.clone());

        // Queue resource types from the search registry to ensure they're generated
        // (they may not be in the model provider's resource list if schemas are partially loaded)
        let search_resource_types = self.search_registry.list_resource_types();
        type_generator.queue_types(search_resource_types.iter());

        // Queue internal resource types (User, Client, AccessPolicy, etc.)
        // These are OctoFHIR-specific resources that may not be in search registry
        debug!(
            internal_types = ?INTERNAL_RESOURCE_TYPES,
            "Queuing internal resource types for GraphQL schema"
        );
        type_generator.queue_types(INTERNAL_RESOURCE_TYPES.iter());

        let mut fhir_types = type_generator.generate_all_types().await?;

        debug!(count = fhir_types.len(), "Generated FHIR types");

        // Collect resource type names for Reference/union registration
        let resource_type_names: Vec<String> = fhir_types.keys().cloned().collect();

        // Add reverse reference fields to resource types
        self.add_reverse_reference_fields(&mut fhir_types);

        // Register all generated types
        for (type_name, _object) in &fhir_types {
            trace!(type_name = %type_name, "Registering FHIR type");
        }
        for (_type_name, object) in fhir_types {
            schema_builder = schema_builder.register(object);
        }

        // Register Reference type and AllResources union for polymorphic resolution
        schema_builder = self.register_reference_types(schema_builder, &resource_type_names);

        // Register connection types for each resource
        schema_builder = self.register_connection_types(schema_builder);

        // Register input types and OperationOutcome for mutations
        schema_builder = self.register_mutation_types(schema_builder);

        // Build Query type with resource fields
        let query = self.build_query_type();
        schema_builder = schema_builder.register(query);

        // Build Mutation type with create/update/delete operations
        let mutation = self.build_mutation_type();
        schema_builder = schema_builder.register(mutation);

        // Build Subscription type if enabled and broadcaster is available
        if self.config.subscriptions_enabled {
            if let Some(ref broadcaster) = self.event_broadcaster {
                debug!("Building subscription type for GraphQL schema");

                // Register the ResourceChangeEvent type
                let event_type = create_resource_change_event_type();
                schema_builder = schema_builder.register(event_type);

                // Build and register the Subscription type
                let subscription = build_subscription_type(broadcaster.clone());
                schema_builder = schema_builder.register(subscription);

                debug!("Subscription type registered");
            } else {
                debug!("Subscriptions enabled but no event broadcaster provided, skipping");
            }
        }

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

    /// Registers the Reference type and AllResources union for polymorphic reference resolution.
    fn register_reference_types(
        &self,
        mut builder: SchemaBuilder,
        resource_types: &[String],
    ) -> SchemaBuilder {
        // Only register if we have resource types
        if resource_types.is_empty() {
            trace!("No resource types for Reference type registration");
            return builder;
        }

        // Register the AllResources union type
        let union = create_all_resources_union(resource_types);
        trace!(
            member_count = resource_types.len(),
            "Registering AllResources union"
        );
        builder = builder.register(union);

        // Register the Reference type with resource resolution field
        let reference = create_reference_type(resource_types);
        trace!("Registering Reference type with resource resolution");
        builder = builder.register(reference);

        builder
    }

    /// Adds reverse reference fields to resource types.
    ///
    /// For each reference search parameter, this adds a field to the target resource types
    /// that allows querying back to the source resources.
    ///
    /// For example, if Observation has a `subject` search parameter that targets Patient,
    /// this adds an `ObservationList_subject` field to the Patient type.
    fn add_reverse_reference_fields(
        &self,
        fhir_types: &mut std::collections::HashMap<String, Object>,
    ) {
        use std::collections::HashMap;

        // Build a map of target_type -> Vec<(source_type, param_name)>
        let mut reverse_refs: HashMap<String, Vec<(String, String)>> = HashMap::new();

        // Get all resource types with search parameters
        let resource_types = self.search_registry.list_resource_types();

        for source_type in &resource_types {
            let params = self.search_registry.get_all_for_type(source_type);

            for param in params {
                if param.param_type == SearchParameterType::Reference {
                    // This is a reference parameter - get its targets
                    for target_type in &param.target {
                        reverse_refs
                            .entry(target_type.clone())
                            .or_default()
                            .push((source_type.to_string(), param.code.clone()));
                    }
                }
            }
        }

        debug!(
            target_count = reverse_refs.len(),
            "Adding reverse reference fields to resource types"
        );

        // Now add fields to each target type
        for (target_type, refs) in reverse_refs {
            if let Some(object) = fhir_types.remove(&target_type) {
                let mut updated_object = object;

                for (source_type, param_name) in refs {
                    // Create field name like "ObservationList_subject"
                    let field_name =
                        format!("{}List_{}", source_type, param_name.replace('-', "_"));

                    trace!(
                        target_type = %target_type,
                        source_type = %source_type,
                        param_name = %param_name,
                        field_name = %field_name,
                        "Adding reverse reference field"
                    );

                    // Create the resolver
                    let resolver = NestedReverseReferenceResolver::resolve(
                        source_type.clone(),
                        param_name.clone(),
                        target_type.clone(),
                    );

                    // Create the field with search arguments
                    let mut field =
                        Field::new(&field_name, TypeRef::named_list(&source_type), resolver)
                            .description(format!(
                                "Find all {} resources where {} references this {}",
                                source_type, param_name, target_type
                            ));

                    // Add common search arguments
                    field = field.argument(InputValue::new("_count", TypeRef::named(TypeRef::INT)));
                    field =
                        field.argument(InputValue::new("_offset", TypeRef::named(TypeRef::INT)));
                    field =
                        field.argument(InputValue::new("_sort", TypeRef::named(TypeRef::STRING)));

                    // Add search parameters for the source type
                    let source_params = self.search_registry.get_all_for_type(&source_type);
                    for source_param in source_params {
                        // Skip the reference param itself - it's implicit
                        if source_param.code == param_name {
                            continue;
                        }

                        let graphql_name = source_param.code.replace('-', "_");
                        let input = InputValue::new(&graphql_name, TypeRef::named(TypeRef::STRING));
                        field = field.argument(input);
                    }

                    updated_object = updated_object.field(field);
                }

                fhir_types.insert(target_type.clone(), updated_object);
            }
        }
    }

    /// Registers connection and edge types for pagination.
    fn register_connection_types(&self, mut builder: SchemaBuilder) -> SchemaBuilder {
        // Get all resource types with search parameters
        let resource_types = self.search_registry.list_resource_types();

        for resource_type in &resource_types {
            builder = self.register_connection_type_for(builder, resource_type);
        }

        // Also register connection types for internal resource types
        for resource_type in INTERNAL_RESOURCE_TYPES {
            if !resource_types.contains(resource_type) {
                builder = self.register_connection_type_for(builder, resource_type);
            }
        }

        builder
    }

    /// Registers Edge and Connection types for a specific resource type.
    fn register_connection_type_for(
        &self,
        mut builder: SchemaBuilder,
        resource_type: &str,
    ) -> SchemaBuilder {
        // Create Edge type
        let edge_type_name = format!("{}Edge", resource_type);
        let edge = Object::new(&edge_type_name)
            .description(format!("Edge type for {} connection", resource_type))
            .field(
                Field::new("resource", TypeRef::named_nn(resource_type), |ctx| {
                    FieldFuture::new(async move {
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
                    Ok(Some(Value::String(env!("CARGO_PKG_VERSION").to_string())))
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
        for resource_type in &resource_types {
            query = self.add_resource_query_fields(query, resource_type);
        }

        // Also add query fields for internal resource types (User, Client, AccessPolicy, etc.)
        // These are OctoFHIR-specific resources that may not have search parameters
        for resource_type in INTERNAL_RESOURCE_TYPES {
            // Skip if already added from search registry
            if !resource_types.contains(resource_type) {
                debug!(resource_type = %resource_type, "Adding internal resource query fields");
                query = self.add_resource_query_fields(query, resource_type);
            }
        }

        query
    }

    /// Adds query fields for a specific resource type.
    fn add_resource_query_fields(&self, mut query: Object, resource_type: &str) -> Object {
        let resource_type_owned = resource_type.to_string();

        // 1. Single resource read: Patient(_id: ID!): Patient
        let read_field_name = resource_type;
        let read_resolver = ReadResolver::resolve(resource_type_owned.clone());

        // Use the actual resource type (Patient, Observation, etc.)
        let read_field = Field::new(
            read_field_name,
            TypeRef::named(resource_type),
            read_resolver,
        )
        .argument(InputValue::new("_id", TypeRef::named_nn(TypeRef::ID)))
        .description(format!("Read a single {} resource by ID", resource_type));

        query = query.field(read_field);
        trace!(resource_type = %resource_type, "Added read query field");

        // 2. List query: PatientList(...): [Patient!]!
        let list_field_name = format!("{}List", resource_type);
        let search_resolver = SearchResolver::resolve(resource_type_owned.clone());

        // Use the actual resource type in the list
        let mut list_field = Field::new(
            &list_field_name,
            TypeRef::named_nn_list_nn(resource_type),
            search_resolver,
        )
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
        connection_field =
            connection_field.argument(InputValue::new("cursor", TypeRef::named(TypeRef::STRING)));

        query = query.field(connection_field);
        trace!(resource_type = %resource_type, "Added connection query field");

        query
    }

    /// Adds search parameter arguments to a field.
    ///
    /// Maps FHIR search parameter types to appropriate GraphQL input types.
    /// All search parameters accept either a single value or a list of values
    /// for OR logic (per FHIR GraphQL spec).
    ///
    /// Type mapping:
    /// - Number: String (allows prefixes like gt100, le50)
    /// - Date: String (allows prefixes and ranges like ge2020-01-01)
    /// - String: String
    /// - Token: String (format: [system|]code)
    /// - Reference: String (format: [ResourceType/]id or URL)
    /// - Composite: String (format: param1$value1$param2$value2)
    /// - Quantity: String (format: [prefix]number[|system|code])
    /// - Uri: String
    /// - Special: String
    fn add_search_arguments(&self, mut field: Field, resource_type: &str) -> Field {
        // Get all search parameters for this resource type
        let params = self.search_registry.get_all_for_type(resource_type);

        for param in params {
            // Convert FHIR param code to GraphQL-safe name
            // GraphQL doesn't allow hyphens in field names
            let graphql_name = param.code.replace('-', "_");

            // All FHIR search parameters use String type because:
            // - Date/Number params support prefixes (ge, le, gt, lt, eq, ne, sa, eb, ap)
            // - Token params use system|code format
            // - Reference params can be relative or absolute URLs
            // - Quantity params include value|system|code
            //
            // We use list type to support OR logic: gender: ["male", "female"]
            // GraphQL coerces single values to lists automatically
            let type_ref = TypeRef::named_list(TypeRef::STRING);

            // Add description based on parameter type for better documentation
            let description = match param.param_type {
                SearchParameterType::Number => {
                    format!("Search by {} (number). Supports prefixes: eq, ne, lt, le, gt, ge, sa, eb, ap", param.code)
                }
                SearchParameterType::Date => {
                    format!("Search by {} (date/time). Supports prefixes and partial dates", param.code)
                }
                SearchParameterType::String => {
                    format!("Search by {} (string). Supports :exact, :contains modifiers", param.code)
                }
                SearchParameterType::Token => {
                    format!("Search by {} (token). Format: [system|]code", param.code)
                }
                SearchParameterType::Reference => {
                    format!("Search by {} (reference). Format: [Type/]id or URL", param.code)
                }
                SearchParameterType::Composite => {
                    format!("Search by {} (composite). Format: param1$value1$param2$value2", param.code)
                }
                SearchParameterType::Quantity => {
                    format!("Search by {} (quantity). Format: [prefix]value[|system|code]", param.code)
                }
                SearchParameterType::Uri => {
                    format!("Search by {} (URI)", param.code)
                }
                SearchParameterType::Special => {
                    format!("Search by {} (special)", param.code)
                }
            };

            let input = InputValue::new(&graphql_name, type_ref).description(description);
            field = field.argument(input);
        }

        // Add common pagination/control arguments
        field = field.argument(
            InputValue::new("_count", TypeRef::named(TypeRef::INT))
                .description("Maximum number of results to return"),
        );
        field = field.argument(
            InputValue::new("_offset", TypeRef::named(TypeRef::INT))
                .description("Number of results to skip"),
        );
        field = field.argument(
            InputValue::new("_sort", TypeRef::named(TypeRef::STRING))
                .description("Sort order. Prefix with - for descending. Example: -date,name"),
        );
        field = field.argument(
            InputValue::new("_filter", TypeRef::named(TypeRef::STRING))
                .description("FHIRPath filter expression for complex queries"),
        );

        // Add _total for controlling total count behavior
        field = field.argument(
            InputValue::new("_total", TypeRef::named(TypeRef::STRING))
                .description("Total count mode: accurate, estimate, or none"),
        );

        // Add _reference for reverse reference queries (FHIR GraphQL spec)
        // This specifies which reference search parameter to use when finding
        // resources that reference the focused resource
        field = field.argument(
            InputValue::new("_reference", TypeRef::named(TypeRef::STRING))
                .description("Reference parameter name for reverse reference queries"),
        );

        field
    }

    /// Registers types needed for mutations.
    ///
    /// This includes:
    /// - JSON scalar for input data
    /// - OperationOutcome types for error/success responses
    /// - Input types for each resource type
    fn register_mutation_types(&self, mut builder: SchemaBuilder) -> SchemaBuilder {
        // Register JSON scalar for accepting arbitrary JSON input
        let json_scalar = create_json_scalar();
        builder = builder.register(json_scalar);

        // Register OperationOutcome types for mutation responses
        let outcome_issue = create_operation_outcome_issue_type();
        builder = builder.register(outcome_issue);

        // Create and register OperationOutcome
        let outcome = Object::new("OperationOutcome")
            .description("Information about the outcome of an operation")
            .field(
                Field::new("resourceType", TypeRef::named_nn(TypeRef::STRING), |_| {
                    FieldFuture::new(async move {
                        Ok(Some(Value::String("OperationOutcome".to_string())))
                    })
                })
                .description("Resource type (always 'OperationOutcome')"),
            )
            .field(
                Field::new("id", TypeRef::named(TypeRef::STRING), |ctx| {
                    FieldFuture::new(async move {
                        if let Some(Value::Object(obj)) = ctx.parent_value.as_value()
                            && let Some(v) = obj.get(&async_graphql::Name::new("id"))
                        {
                            return Ok(Some(v.clone()));
                        }
                        Ok(None)
                    })
                })
                .description("Logical id of this outcome"),
            )
            .field(
                Field::new(
                    "issue",
                    TypeRef::named_nn_list_nn("OperationOutcomeIssue"),
                    |ctx| {
                        FieldFuture::new(async move {
                            if let Some(Value::Object(obj)) = ctx.parent_value.as_value()
                                && let Some(v) = obj.get(&async_graphql::Name::new("issue"))
                            {
                                return Ok(Some(v.clone()));
                            }
                            Ok(Some(Value::List(vec![])))
                        })
                    },
                )
                .description("Issues that occurred during the operation"),
            );
        builder = builder.register(outcome);

        // Register input types for each resource type
        let resource_types = self.search_registry.list_resource_types();
        for resource_type in &resource_types {
            let input = InputTypeGenerator::create_resource_input(resource_type);
            trace!(resource_type = %resource_type, "Registering input type");
            builder = builder.register(input);
        }

        // Also register input types for internal resource types (User, Client, AccessPolicy, etc.)
        for resource_type in INTERNAL_RESOURCE_TYPES {
            if !resource_types.contains(resource_type) {
                let input = InputTypeGenerator::create_resource_input(resource_type);
                trace!(resource_type = %resource_type, "Registering internal resource input type");
                builder = builder.register(input);
            }
        }

        debug!(
            count = resource_types.len(),
            "Registered mutation input types"
        );

        builder
    }

    /// Builds the Mutation root type with create/update/delete operations.
    fn build_mutation_type(&self) -> Object {
        let mut mutation = Object::new("Mutation")
            .description("FHIR GraphQL Mutation root - create, update, and delete operations");

        // Get all resource types
        let resource_types = self.search_registry.list_resource_types();

        // Add placeholder if no resource types (GraphQL requires at least one field)
        if resource_types.is_empty() {
            mutation = mutation.field(
                Field::new("_placeholder", TypeRef::named(TypeRef::STRING), |_| {
                    FieldFuture::new(async { Ok(None::<Value>) })
                })
                .description("Placeholder field - no resource types configured"),
            );
            return mutation;
        }

        for resource_type in &resource_types {
            mutation = self.add_resource_mutation_fields(mutation, resource_type);
        }

        // Also add mutations for internal resource types (User, Client, AccessPolicy, etc.)
        for resource_type in INTERNAL_RESOURCE_TYPES {
            // Skip if already added from search registry
            if !resource_types.contains(resource_type) {
                debug!(resource_type = %resource_type, "Adding internal resource mutation fields");
                mutation = self.add_resource_mutation_fields(mutation, resource_type);
            }
        }

        debug!("Built Mutation type with CRUD operations");
        mutation
    }

    /// Adds mutation fields (Create, Update, Delete) for a specific resource type.
    fn add_resource_mutation_fields(&self, mut mutation: Object, resource_type: &str) -> Object {
        // Create mutation: PatientCreate(res: PatientInput!): Patient
        let create_field_name = format!("{}Create", resource_type);
        let input_type_name = format!("{}Input", resource_type);

        let create_resolver = CreateResolver::resolve(resource_type.to_string());
        let create_field = Field::new(
            &create_field_name,
            TypeRef::named(resource_type),
            create_resolver,
        )
        .description(format!("Create a new {} resource", resource_type))
        .argument(
            InputValue::new("res", TypeRef::named_nn(&input_type_name))
                .description("The resource to create"),
        );

        mutation = mutation.field(create_field);

        // Update mutation: PatientUpdate(id: ID!, res: PatientInput!, ifMatch: String): Patient
        let update_field_name = format!("{}Update", resource_type);

        let update_resolver = UpdateResolver::resolve(resource_type.to_string());
        let update_field = Field::new(
            &update_field_name,
            TypeRef::named(resource_type),
            update_resolver,
        )
        .description(format!("Update an existing {} resource", resource_type))
        .argument(
            InputValue::new("id", TypeRef::named_nn(TypeRef::ID))
                .description("The ID of the resource to update"),
        )
        .argument(
            InputValue::new("res", TypeRef::named_nn(&input_type_name))
                .description("The updated resource data"),
        )
        .argument(
            InputValue::new("ifMatch", TypeRef::named(TypeRef::STRING))
                .description("Version for optimistic locking (e.g., 'W/\"1\"')"),
        );

        mutation = mutation.field(update_field);

        // Delete mutation: PatientDelete(id: ID!): OperationOutcome
        let delete_field_name = format!("{}Delete", resource_type);

        let delete_resolver = DeleteResolver::resolve(resource_type.to_string());
        let delete_field = Field::new(
            &delete_field_name,
            TypeRef::named("OperationOutcome"),
            delete_resolver,
        )
        .description(format!("Delete a {} resource", resource_type))
        .argument(
            InputValue::new("id", TypeRef::named_nn(TypeRef::ID))
                .description("The ID of the resource to delete"),
        );

        mutation = mutation.field(delete_field);

        trace!(
            resource_type = %resource_type,
            "Registered mutations for resource type"
        );

        mutation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_fhir_model::provider::FhirVersion;
    use octofhir_fhirschema::{FhirSchemaModelProvider, get_schemas};

    fn test_model_provider() -> DynModelProvider {
        // Use embedded FHIR R4 schemas for real type generation
        // Note: R4B package only has 5 extension StructureDefinitions, R4 has all core types
        let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4).clone();
        let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);
        Arc::new(provider)
    }

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
        let model_provider = test_model_provider();

        let builder =
            FhirSchemaBuilder::new(registry, model_provider, SchemaBuilderConfig::default());

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
        let model_provider = test_model_provider();
        let builder =
            FhirSchemaBuilder::new(registry, model_provider, SchemaBuilderConfig::default());

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

        let model_provider = test_model_provider();
        let builder = FhirSchemaBuilder::new(
            Arc::new(registry),
            model_provider,
            SchemaBuilderConfig::default(),
        );

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
        let model_provider = test_model_provider();
        let config = SchemaBuilderConfig {
            introspection_enabled: false,
            ..Default::default()
        };

        let builder = FhirSchemaBuilder::new(registry, model_provider, config);
        let result = builder.build().await;

        assert!(
            result.is_ok(),
            "Schema should build with introspection disabled"
        );
    }
}
