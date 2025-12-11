//! Connection-based pagination resolver.
//!
//! Implements resolvers for queries like `PatientConnection(cursor: "abc")` that
//! provide cursor-based pagination with navigation links.

use async_graphql::dynamic::{FieldFuture, ResolverContext};
use async_graphql::Value;
use octofhir_storage::{SearchParams, TotalMode};
use tracing::{debug, warn};

use super::{get_graphql_context, json_to_graphql_value};

/// Resolver for connection-based pagination.
pub struct ConnectionResolver;

/// Cursor encoding/decoding utilities.
mod cursor {
    use base64::Engine;
    use serde::{Deserialize, Serialize};

    /// Cursor data encoded in the cursor string.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct CursorData {
        /// Offset into the result set.
        pub offset: u32,
        /// Page size for this pagination.
        pub page_size: u32,
    }

    impl CursorData {
        pub fn new(offset: u32, page_size: u32) -> Self {
            Self { offset, page_size }
        }

        /// Encode cursor data to a base64 string.
        pub fn encode(&self) -> String {
            let json = serde_json::to_string(self).unwrap_or_default();
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
        }

        /// Decode cursor data from a base64 string.
        pub fn decode(cursor: &str) -> Option<Self> {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(cursor)
                .ok()?;
            let json = String::from_utf8(bytes).ok()?;
            serde_json::from_str(&json).ok()
        }
    }
}

/// Helper to create a GraphQL Name.
fn make_name(s: &str) -> async_graphql::Name {
    async_graphql::Name::new(s)
}

impl ConnectionResolver {
    /// Creates a resolver function for connection queries.
    ///
    /// This is used to create the `PatientConnection(...)` style query fields.
    pub fn resolve(resource_type: String) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();
            FieldFuture::new(async move {
                debug!(
                    resource_type = %resource_type,
                    "Resolving connection query"
                );

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Parse cursor if provided
                let cursor_data = ctx
                    .args
                    .get("cursor")
                    .and_then(|v| v.string().ok())
                    .and_then(cursor::CursorData::decode);

                // Get page size from _count or default
                let page_size = ctx
                    .args
                    .get("_count")
                    .and_then(|v| v.i64().ok())
                    .map(|n| n as u32)
                    .unwrap_or(gql_ctx.search_config.default_count as u32);

                // Calculate offset from cursor or start at 0
                let offset = cursor_data.as_ref().map(|c| c.offset).unwrap_or(0);

                // Build search parameters
                let mut params = SearchParams::new()
                    .with_count(page_size)
                    .with_offset(offset)
                    .with_total(TotalMode::Accurate);

                for (key, value) in ctx.args.iter() {
                    // Skip pagination-specific args
                    if key == "cursor" || key == "_count" || key == "_offset" {
                        continue;
                    }

                    let fhir_key = if key.starts_with('_') {
                        key.to_string()
                    } else {
                        key.replace('_', "-")
                    };

                    if let Ok(s) = value.string() {
                        params = params.with_param(&fhir_key, s);
                    }
                }

                debug!(
                    resource_type = %resource_type,
                    offset = offset,
                    page_size = page_size,
                    "Executing connection search"
                );

                // Execute search using FhirStorage trait
                let result = gql_ctx
                    .storage
                    .search(&resource_type, &params)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Storage error during search");
                        async_graphql::Error::new(format!("Search failed: {}", e))
                    })?;

                let total = result.total.unwrap_or(0);
                let entries_count = result.entries.len() as u32;

                // Build edges
                let edges: Vec<Value> = result
                    .entries
                    .into_iter()
                    .map(|stored| {
                        let resource_value = json_to_graphql_value(stored.resource);
                        let mut edge = async_graphql::indexmap::IndexMap::new();
                        edge.insert(make_name("resource"), resource_value);
                        edge.insert(make_name("mode"), Value::String("match".to_string()));
                        Value::Object(edge)
                    })
                    .collect();

                // Build navigation cursors
                let first_cursor = if total > 0 {
                    Some(cursor::CursorData::new(0, page_size).encode())
                } else {
                    None
                };

                let previous_cursor = if offset > 0 {
                    let prev_offset = offset.saturating_sub(page_size);
                    Some(cursor::CursorData::new(prev_offset, page_size).encode())
                } else {
                    None
                };

                let next_cursor = if offset + entries_count < total {
                    Some(cursor::CursorData::new(offset + page_size, page_size).encode())
                } else {
                    None
                };

                let last_cursor = if total > 0 {
                    let last_offset = ((total - 1) / page_size) * page_size;
                    Some(cursor::CursorData::new(last_offset, page_size).encode())
                } else {
                    None
                };

                // Build connection response
                let mut connection = async_graphql::indexmap::IndexMap::new();
                connection.insert(make_name("count"), Value::Number((total as i64).into()));
                connection.insert(make_name("offset"), Value::Number((offset as i64).into()));
                connection.insert(make_name("pageSize"), Value::Number((page_size as i64).into()));
                connection.insert(make_name("edges"), Value::List(edges));

                // Add navigation cursors
                connection.insert(
                    make_name("first"),
                    first_cursor.map(Value::String).unwrap_or(Value::Null),
                );
                connection.insert(
                    make_name("previous"),
                    previous_cursor.map(Value::String).unwrap_or(Value::Null),
                );
                connection.insert(
                    make_name("next"),
                    next_cursor.map(Value::String).unwrap_or(Value::Null),
                );
                connection.insert(
                    make_name("last"),
                    last_cursor.map(Value::String).unwrap_or(Value::Null),
                );

                debug!(
                    resource_type = %resource_type,
                    total = total,
                    returned = entries_count,
                    "Connection query completed"
                );

                Ok(Some(Value::Object(connection)))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cursor::CursorData;

    #[test]
    fn test_cursor_encode_decode() {
        let cursor = CursorData::new(10, 20);
        let encoded = cursor.encode();

        let decoded = CursorData::decode(&encoded).expect("Should decode");
        assert_eq!(decoded.offset, 10);
        assert_eq!(decoded.page_size, 20);
    }

    #[test]
    fn test_cursor_decode_invalid() {
        assert!(CursorData::decode("not-valid-base64!!!").is_none());
        assert!(CursorData::decode("").is_none());
    }

    #[test]
    fn test_cursor_roundtrip() {
        for offset in [0, 1, 100, 999] {
            for page_size in [10, 25, 50, 100] {
                let original = CursorData::new(offset, page_size);
                let encoded = original.encode();
                let decoded = CursorData::decode(&encoded).unwrap();

                assert_eq!(decoded.offset, offset);
                assert_eq!(decoded.page_size, page_size);
            }
        }
    }
}
