//! GraphQL resolvers for FHIR resources.
//!
//! This module provides the resolver implementations for GraphQL queries:
//! - `read`: Single resource queries (e.g., `Patient(_id: "123")`)
//! - `search`: List queries with search parameters (e.g., `PatientList(name: "John")`)
//! - `connection`: Cursor-based pagination (e.g., `PatientConnection(...)`)

mod connection;
mod read;
mod reverse_reference;
mod search;

pub use connection::ConnectionResolver;
pub use read::ReadResolver;
pub use reverse_reference::ReverseReferenceResolver;
pub use search::SearchResolver;

use async_graphql::dynamic::ResolverContext;
use async_graphql::{Error as GraphQLError, Value};

use crate::context::GraphQLContext;

/// Helper to extract GraphQL context from resolver context.
pub(crate) fn get_graphql_context<'a>(ctx: &'a ResolverContext<'_>) -> Result<&'a GraphQLContext, GraphQLError> {
    ctx.data::<GraphQLContext>()
        .map_err(|_| GraphQLError::new("GraphQL context not available"))
}

/// Convert a serde_json::Value to async_graphql::Value.
pub(crate) fn json_to_graphql_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(
                    async_graphql::Number::from_f64(f).unwrap_or_else(|| async_graphql::Number::from(0)),
                )
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            Value::List(arr.into_iter().map(json_to_graphql_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: async_graphql::indexmap::IndexMap<async_graphql::Name, Value> = obj
                .into_iter()
                .map(|(k, v)| (async_graphql::Name::new(k), json_to_graphql_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}
