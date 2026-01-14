//! AppSubscription reconciliation logic.

use std::collections::{HashMap, HashSet};

use octofhir_api::ApiError;
use octofhir_storage::{DynStorage, SearchParams, StoredResource};

use crate::gateway::types::{AppSubscription, InlineSubscription, Reference};

use super::ReconcileResult;

/// Reconcile AppSubscriptions based on App.subscriptions[].
///
/// This function performs a three-way diff between existing AppSubscriptions
/// in the database and the subscriptions defined in the App resource:
/// - Creates new subscriptions that don't exist
/// - Updates existing subscriptions if their definitions changed
/// - Deletes subscriptions that are no longer in the App
///
/// # Arguments
///
/// * `storage` - Storage backend for FHIR resources
/// * `app_id` - ID of the App resource
/// * `app_name` - Name of the App (for display in references)
/// * `inline_subscriptions` - Subscriptions from App.subscriptions[]
///
/// # Returns
///
/// `ReconcileResult` containing lists of created, updated, and deleted subscription IDs.
pub async fn reconcile_subscriptions(
    storage: &DynStorage,
    app_id: &str,
    app_name: &str,
    inline_subscriptions: &[InlineSubscription],
) -> Result<ReconcileResult, ApiError> {
    let mut result = ReconcileResult::default();

    // 1. Load existing AppSubscriptions for this app
    let current_subs = load_app_subscriptions(storage, app_id).await?;
    let current_map: HashMap<String, AppSubscription> = current_subs
        .into_iter()
        .map(|sub| {
            let sub_id = extract_sub_id(&sub.id);
            (sub_id, sub)
        })
        .collect();

    // 2. Build manifest subscriptions map
    let manifest_map: HashMap<String, &InlineSubscription> = inline_subscriptions
        .iter()
        .map(|sub| (sub.id.clone(), sub))
        .collect();

    let manifest_ids: HashSet<&String> = manifest_map.keys().collect();
    let current_ids: HashSet<String> = current_map.keys().cloned().collect();
    let current_ids_ref: HashSet<&String> = current_ids.iter().collect();

    // 3. CREATE new subscriptions
    for sub_id in manifest_ids.difference(&current_ids_ref) {
        let inline_sub = manifest_map[*sub_id];
        let app_sub = build_app_subscription(app_id, app_name, inline_sub)?;
        let resource_json = serde_json::to_value(&app_sub).map_err(|e| {
            ApiError::internal(format!("Failed to serialize AppSubscription: {}", e))
        })?;
        storage
            .create(&resource_json)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to create AppSubscription: {}", e)))?;
        result.created.push((*sub_id).clone());
    }

    // 4. UPDATE changed subscriptions
    for sub_id in manifest_ids.intersection(&current_ids_ref) {
        let inline_sub = manifest_map[*sub_id];
        let current_sub = &current_map[*sub_id];

        if needs_update(current_sub, inline_sub) {
            let app_sub = build_app_subscription(app_id, app_name, inline_sub)?;
            let resource_json = serde_json::to_value(&app_sub).map_err(|e| {
                ApiError::internal(format!("Failed to serialize AppSubscription: {}", e))
            })?;
            storage.update(&resource_json, None).await.map_err(|e| {
                ApiError::internal(format!("Failed to update AppSubscription: {}", e))
            })?;
            result.updated.push((*sub_id).clone());
        }
    }

    // 5. DELETE removed subscriptions
    for sub_id in current_ids_ref.difference(&manifest_ids) {
        let full_id = format!("{}-{}", app_id, sub_id);
        storage
            .delete("AppSubscription", &full_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to delete AppSubscription: {}", e)))?;
        result.deleted.push((*sub_id).clone());
    }

    tracing::info!(
        app_id = %app_id,
        created = result.created.len(),
        updated = result.updated.len(),
        deleted = result.deleted.len(),
        "AppSubscriptions reconciled"
    );

    Ok(result)
}

/// Load all AppSubscriptions for a given App from storage.
async fn load_app_subscriptions(
    storage: &DynStorage,
    app_id: &str,
) -> Result<Vec<AppSubscription>, ApiError> {
    let search_params = SearchParams::new().with_count(1000);
    let search_result = storage
        .search("AppSubscription", &search_params)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to search AppSubscriptions: {}", e)))?;

    let app_ref = format!("App/{}", app_id);
    let subscriptions: Vec<AppSubscription> = search_result
        .entries
        .into_iter()
        .filter_map(|stored: StoredResource| {
            serde_json::from_value::<AppSubscription>(stored.resource).ok()
        })
        .filter(|sub| {
            sub.app
                .reference
                .as_ref()
                .map(|r| r == &app_ref)
                .unwrap_or(false)
        })
        .collect();

    Ok(subscriptions)
}

/// Build an AppSubscription from an InlineSubscription.
fn build_app_subscription(
    app_id: &str,
    app_name: &str,
    inline_sub: &InlineSubscription,
) -> Result<AppSubscription, ApiError> {
    Ok(AppSubscription {
        id: Some(format!("{}-{}", app_id, inline_sub.id)),
        resource_type: "AppSubscription".to_string(),
        app: Reference {
            reference: Some(format!("App/{}", app_id)),
            display: Some(app_name.to_string()),
        },
        trigger: inline_sub.trigger.clone(),
        channel: inline_sub.channel.clone(),
        notification: inline_sub.notification.clone(),
        active: true,
    })
}

/// Check if an AppSubscription needs to be updated based on InlineSubscription.
fn needs_update(current: &AppSubscription, inline_sub: &InlineSubscription) -> bool {
    // Compare trigger, channel, and notification
    // Note: We use Debug format for comparison as a simple approach
    // In production, you might want to implement PartialEq for these types
    format!("{:?}", current.trigger) != format!("{:?}", inline_sub.trigger)
        || format!("{:?}", current.channel) != format!("{:?}", inline_sub.channel)
        || format!("{:?}", current.notification) != format!("{:?}", inline_sub.notification)
}

/// Extract subscription ID from full AppSubscription ID.
///
/// Full ID format: "{app_id}-{subscription_id}"
/// Example: "psychportal-patient-created" -> "patient-created"
fn extract_sub_id(full_id: &Option<String>) -> String {
    full_id
        .as_ref()
        .and_then(|id| {
            // Find the first '-' and take everything after it
            id.find('-').map(|idx| &id[idx + 1..])
        })
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_platform::{SubscriptionEvent, SubscriptionTrigger};

    #[test]
    fn test_extract_sub_id() {
        assert_eq!(
            extract_sub_id(&Some("psychportal-patient-created".to_string())),
            "patient-created"
        );
        assert_eq!(
            extract_sub_id(&Some("app-id-with-dashes-sub-id".to_string())),
            "id-with-dashes-sub-id"
        );
        assert_eq!(extract_sub_id(&Some("no-dash".to_string())), "dash");
        assert_eq!(extract_sub_id(&None), "");
    }

    #[test]
    fn test_build_app_subscription() {
        let inline_sub = InlineSubscription {
            id: "test-sub".to_string(),
            trigger: SubscriptionTrigger {
                resource_type: "Patient".to_string(),
                event: SubscriptionEvent::Create,
                fhirpath: None,
            },
            channel: None,
            notification: None,
        };

        let app_sub = build_app_subscription("test-app", "Test App", &inline_sub).unwrap();

        assert_eq!(app_sub.id, Some("test-app-test-sub".to_string()));
        assert_eq!(app_sub.resource_type, "AppSubscription");
        assert_eq!(app_sub.app.reference, Some("App/test-app".to_string()));
        assert_eq!(app_sub.app.display, Some("Test App".to_string()));
        assert_eq!(app_sub.trigger.resource_type, "Patient");
        assert_eq!(app_sub.active, true);
    }

    #[test]
    fn test_needs_update_no_changes() {
        let inline_sub = InlineSubscription {
            id: "test-sub".to_string(),
            trigger: SubscriptionTrigger {
                resource_type: "Patient".to_string(),
                event: SubscriptionEvent::Create,
                fhirpath: None,
            },
            channel: None,
            notification: None,
        };

        let app_sub = build_app_subscription("test-app", "Test App", &inline_sub).unwrap();

        assert!(!needs_update(&app_sub, &inline_sub));
    }

    #[test]
    fn test_needs_update_trigger_changed() {
        let inline_sub = InlineSubscription {
            id: "test-sub".to_string(),
            trigger: SubscriptionTrigger {
                resource_type: "Patient".to_string(),
                event: SubscriptionEvent::Create,
                fhirpath: None,
            },
            channel: None,
            notification: None,
        };

        let mut app_sub = build_app_subscription("test-app", "Test App", &inline_sub).unwrap();

        // Change trigger
        app_sub.trigger.resource_type = "Observation".to_string();

        assert!(needs_update(&app_sub, &inline_sub));
    }
}
