//! Keeps sign-in in step with the `IdentityProvider` resources.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_auth::federation::IdpAuthService;
use octofhir_auth::federation::resources::IdentityProviderResource;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};

/// Registers a provider when its resource changes, however it was changed.
#[derive(Clone)]
pub struct IdentityProviderHook {
    service: Arc<IdpAuthService>,
}

impl IdentityProviderHook {
    pub fn new(service: Arc<IdpAuthService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl ResourceHook for IdentityProviderHook {
    fn name(&self) -> &str {
        "identity_provider"
    }

    fn resource_types(&self) -> &[&str] {
        &["IdentityProvider"]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let resource = match (&event.event_type, &event.resource) {
            (ResourceEventType::Deleted, _) | (_, None) => {
                self.service.unregister_provider(&event.resource_id).await;
                return Ok(());
            }
            (_, Some(resource)) => resource,
        };
        let parsed = serde_json::from_value::<IdentityProviderResource>((**resource).clone())
            .map_err(|error| error.to_string())
            .and_then(|provider| provider.to_config().map_err(|error| error.to_string()));
        match parsed {
            Ok(provider) => self.service.register_provider(provider).await,
            // A provider that cannot be read must not stay on what it said before.
            Err(error) => {
                tracing::error!(id = %event.resource_id, %error, "Unusable identity provider");
                self.service.unregister_provider(&event.resource_id).await;
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for IdentityProviderHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityProviderHook").finish()
    }
}
