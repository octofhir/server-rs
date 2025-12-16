//! GraphQL subscription field definitions.

use async_graphql::dynamic::{
    InputValue, Object, Subscription, SubscriptionField, SubscriptionFieldFuture, TypeRef,
};
use async_graphql::Value;
use async_stream::stream;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

use super::events::{ResourceChangeEvent, ResourceEventBroadcaster, ResourceEventType};

/// Builds the GraphQL Subscription type with all subscription fields.
///
/// Creates the following subscription fields:
/// - `resourceCreated(resourceType: String)` - New resources
/// - `resourceUpdated(resourceType: String)` - Updated resources
/// - `resourceDeleted(resourceType: String)` - Deleted resources
/// - `resourceChanged(resourceType: String, eventType: String)` - All changes
pub fn build_subscription_type(broadcaster: Arc<ResourceEventBroadcaster>) -> Subscription {
    let mut subscription = Subscription::new("Subscription");

    // resourceCreated subscription
    subscription = subscription.field(create_resource_created_field(broadcaster.clone()));

    // resourceUpdated subscription
    subscription = subscription.field(create_resource_updated_field(broadcaster.clone()));

    // resourceDeleted subscription
    subscription = subscription.field(create_resource_deleted_field(broadcaster.clone()));

    // resourceChanged subscription (all events)
    subscription = subscription.field(create_resource_changed_field(broadcaster));

    subscription
}

/// Creates the `resourceCreated` subscription field.
fn create_resource_created_field(
    broadcaster: Arc<ResourceEventBroadcaster>,
) -> SubscriptionField {
    SubscriptionField::new(
        "resourceCreated",
        TypeRef::named_nn("ResourceChangeEvent"),
        move |ctx| {
            let broadcaster = broadcaster.clone();
            let resource_type_filter: Option<String> = ctx
                .args
                .get("resourceType")
                .and_then(|v| v.string().ok())
                .map(|s| s.to_string());

            SubscriptionFieldFuture::new(async move {
                let receiver = broadcaster.subscribe();
                debug!(
                    filter = ?resource_type_filter,
                    "Starting resourceCreated subscription"
                );

                let stream = create_filtered_stream(
                    receiver,
                    resource_type_filter,
                    Some(ResourceEventType::Created),
                );

                Ok(stream)
            })
        },
    )
    .description("Subscribe to resource creation events")
    .argument(
        InputValue::new("resourceType", TypeRef::named(TypeRef::STRING))
            .description("Filter by resource type (e.g., 'Patient', 'Observation')"),
    )
}

/// Creates the `resourceUpdated` subscription field.
fn create_resource_updated_field(
    broadcaster: Arc<ResourceEventBroadcaster>,
) -> SubscriptionField {
    SubscriptionField::new(
        "resourceUpdated",
        TypeRef::named_nn("ResourceChangeEvent"),
        move |ctx| {
            let broadcaster = broadcaster.clone();
            let resource_type_filter: Option<String> = ctx
                .args
                .get("resourceType")
                .and_then(|v| v.string().ok())
                .map(|s| s.to_string());

            SubscriptionFieldFuture::new(async move {
                let receiver = broadcaster.subscribe();
                debug!(
                    filter = ?resource_type_filter,
                    "Starting resourceUpdated subscription"
                );

                let stream = create_filtered_stream(
                    receiver,
                    resource_type_filter,
                    Some(ResourceEventType::Updated),
                );

                Ok(stream)
            })
        },
    )
    .description("Subscribe to resource update events")
    .argument(
        InputValue::new("resourceType", TypeRef::named(TypeRef::STRING))
            .description("Filter by resource type (e.g., 'Patient', 'Observation')"),
    )
}

/// Creates the `resourceDeleted` subscription field.
fn create_resource_deleted_field(
    broadcaster: Arc<ResourceEventBroadcaster>,
) -> SubscriptionField {
    SubscriptionField::new(
        "resourceDeleted",
        TypeRef::named_nn("ResourceChangeEvent"),
        move |ctx| {
            let broadcaster = broadcaster.clone();
            let resource_type_filter: Option<String> = ctx
                .args
                .get("resourceType")
                .and_then(|v| v.string().ok())
                .map(|s| s.to_string());

            SubscriptionFieldFuture::new(async move {
                let receiver = broadcaster.subscribe();
                debug!(
                    filter = ?resource_type_filter,
                    "Starting resourceDeleted subscription"
                );

                let stream = create_filtered_stream(
                    receiver,
                    resource_type_filter,
                    Some(ResourceEventType::Deleted),
                );

                Ok(stream)
            })
        },
    )
    .description("Subscribe to resource deletion events")
    .argument(
        InputValue::new("resourceType", TypeRef::named(TypeRef::STRING))
            .description("Filter by resource type (e.g., 'Patient', 'Observation')"),
    )
}

/// Creates the `resourceChanged` subscription field that receives all events.
fn create_resource_changed_field(
    broadcaster: Arc<ResourceEventBroadcaster>,
) -> SubscriptionField {
    SubscriptionField::new(
        "resourceChanged",
        TypeRef::named_nn("ResourceChangeEvent"),
        move |ctx| {
            let broadcaster = broadcaster.clone();
            let resource_type_filter: Option<String> = ctx
                .args
                .get("resourceType")
                .and_then(|v| v.string().ok())
                .map(|s| s.to_string());
            let event_type_filter: Option<ResourceEventType> = ctx
                .args
                .get("eventType")
                .and_then(|v| v.string().ok())
                .and_then(|s| match s {
                    "created" => Some(ResourceEventType::Created),
                    "updated" => Some(ResourceEventType::Updated),
                    "deleted" => Some(ResourceEventType::Deleted),
                    _ => None,
                });

            SubscriptionFieldFuture::new(async move {
                let receiver = broadcaster.subscribe();
                debug!(
                    resource_filter = ?resource_type_filter,
                    event_filter = ?event_type_filter,
                    "Starting resourceChanged subscription"
                );

                let stream = create_filtered_stream(receiver, resource_type_filter, event_type_filter);

                Ok(stream)
            })
        },
    )
    .description("Subscribe to all resource change events")
    .argument(
        InputValue::new("resourceType", TypeRef::named(TypeRef::STRING))
            .description("Filter by resource type (e.g., 'Patient', 'Observation')"),
    )
    .argument(
        InputValue::new("eventType", TypeRef::named(TypeRef::STRING))
            .description("Filter by event type: 'created', 'updated', or 'deleted'"),
    )
}

/// Creates a filtered stream from a broadcast receiver.
fn create_filtered_stream(
    mut receiver: broadcast::Receiver<ResourceChangeEvent>,
    resource_type_filter: Option<String>,
    event_type_filter: Option<ResourceEventType>,
) -> impl futures_util::Stream<Item = Result<Value, async_graphql::Error>> + Send {
    stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    // Apply filters
                    if !event.matches_type(resource_type_filter.as_deref()) {
                        trace!(
                            event_type = %event.resource_type,
                            filter = ?resource_type_filter,
                            "Event filtered by resource type"
                        );
                        continue;
                    }
                    if !event.matches_event_type(event_type_filter) {
                        trace!(
                            event_type = ?event.event_type,
                            filter = ?event_type_filter,
                            "Event filtered by event type"
                        );
                        continue;
                    }

                    trace!(
                        event_type = ?event.event_type,
                        resource_type = %event.resource_type,
                        resource_id = %event.resource_id,
                        "Emitting subscription event"
                    );

                    // Convert event to GraphQL Value
                    yield Ok(event_to_graphql_value(&event));
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!(
                        count,
                        "Subscription lagged, some events were dropped"
                    );
                    // Continue receiving
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Subscription channel closed");
                    break;
                }
            }
        }
    }
}

/// Converts a ResourceChangeEvent to a GraphQL Value.
fn event_to_graphql_value(event: &ResourceChangeEvent) -> Value {
    let mut obj = async_graphql::indexmap::IndexMap::new();

    obj.insert(
        async_graphql::Name::new("eventType"),
        Value::String(event.event_type.to_string()),
    );
    obj.insert(
        async_graphql::Name::new("resourceType"),
        Value::String(event.resource_type.clone()),
    );
    obj.insert(
        async_graphql::Name::new("resourceId"),
        Value::String(event.resource_id.clone()),
    );

    if let Some(resource) = &event.resource {
        obj.insert(
            async_graphql::Name::new("resource"),
            json_to_graphql_value(resource.clone()),
        );
    } else {
        obj.insert(async_graphql::Name::new("resource"), Value::Null);
    }

    obj.insert(
        async_graphql::Name::new("timestamp"),
        Value::String(
            event
                .timestamp
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| event.timestamp.to_string()),
        ),
    );

    Value::Object(obj)
}

/// Converts a serde_json::Value to an async_graphql::Value.
fn json_to_graphql_value(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(
                    async_graphql::Number::from_f64(f).unwrap_or(async_graphql::Number::from(0)),
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
            let map: async_graphql::indexmap::IndexMap<_, _> = obj
                .into_iter()
                .map(|(k, v)| (async_graphql::Name::new(k), json_to_graphql_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}

/// Creates the ResourceChangeEvent type for the schema.
pub fn create_resource_change_event_type() -> Object {
    Object::new("ResourceChangeEvent")
        .description("Event emitted when a FHIR resource changes")
        .field(
            async_graphql::dynamic::Field::new(
                "eventType",
                TypeRef::named_nn(TypeRef::STRING),
                |ctx| {
                    async_graphql::dynamic::FieldFuture::new(async move {
                        if let Some(parent) = ctx.parent_value.as_value() {
                            if let Value::Object(obj) = parent {
                                if let Some(v) = obj.get("eventType") {
                                    return Ok(Some(v.clone()));
                                }
                            }
                        }
                        Ok(None)
                    })
                },
            )
            .description("Type of change: 'created', 'updated', or 'deleted'"),
        )
        .field(
            async_graphql::dynamic::Field::new(
                "resourceType",
                TypeRef::named_nn(TypeRef::STRING),
                |ctx| {
                    async_graphql::dynamic::FieldFuture::new(async move {
                        if let Some(parent) = ctx.parent_value.as_value() {
                            if let Value::Object(obj) = parent {
                                if let Some(v) = obj.get("resourceType") {
                                    return Ok(Some(v.clone()));
                                }
                            }
                        }
                        Ok(None)
                    })
                },
            )
            .description("FHIR resource type (e.g., 'Patient', 'Observation')"),
        )
        .field(
            async_graphql::dynamic::Field::new(
                "resourceId",
                TypeRef::named_nn(TypeRef::STRING),
                |ctx| {
                    async_graphql::dynamic::FieldFuture::new(async move {
                        if let Some(parent) = ctx.parent_value.as_value() {
                            if let Value::Object(obj) = parent {
                                if let Some(v) = obj.get("resourceId") {
                                    return Ok(Some(v.clone()));
                                }
                            }
                        }
                        Ok(None)
                    })
                },
            )
            .description("Resource ID"),
        )
        .field(
            async_graphql::dynamic::Field::new(
                "resource",
                TypeRef::named("FhirResource"),
                |ctx| {
                    async_graphql::dynamic::FieldFuture::new(async move {
                        if let Some(parent) = ctx.parent_value.as_value() {
                            if let Value::Object(obj) = parent {
                                if let Some(v) = obj.get("resource") {
                                    return Ok(Some(v.clone()));
                                }
                            }
                        }
                        Ok(None)
                    })
                },
            )
            .description("The resource data (null for deletions)"),
        )
        .field(
            async_graphql::dynamic::Field::new(
                "timestamp",
                TypeRef::named_nn(TypeRef::STRING),
                |ctx| {
                    async_graphql::dynamic::FieldFuture::new(async move {
                        if let Some(parent) = ctx.parent_value.as_value() {
                            if let Value::Object(obj) = parent {
                                if let Some(v) = obj.get("timestamp") {
                                    return Ok(Some(v.clone()));
                                }
                            }
                        }
                        Ok(None)
                    })
                },
            )
            .description("ISO 8601 timestamp of the event"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_to_graphql_value() {
        let event = ResourceChangeEvent::created(
            "Patient",
            "123",
            serde_json::json!({"resourceType": "Patient", "id": "123"}),
        );

        let value = event_to_graphql_value(&event);

        if let Value::Object(obj) = value {
            assert_eq!(
                obj.get("eventType"),
                Some(&Value::String("created".to_string()))
            );
            assert_eq!(
                obj.get("resourceType"),
                Some(&Value::String("Patient".to_string()))
            );
            assert_eq!(
                obj.get("resourceId"),
                Some(&Value::String("123".to_string()))
            );
            assert!(obj.get("resource").is_some());
            assert!(obj.get("timestamp").is_some());
        } else {
            panic!("Expected Object value");
        }
    }

    #[test]
    fn test_build_subscription_type() {
        let broadcaster = ResourceEventBroadcaster::new_shared();
        let subscription = build_subscription_type(broadcaster);
        assert_eq!(subscription.type_name(), "Subscription");
    }
}
