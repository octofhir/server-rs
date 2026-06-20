//! Gateway Custom Operations Provider
//!
//! Provides operation definitions for API Gateway CustomOperation resources.

use std::collections::HashMap;

use octofhir_core::{AppReference, OperationDefinition, OperationProvider, categories};
use octofhir_storage::{DynStorage, SearchParams};
use tracing::{debug, warn};

use crate::gateway::types::{App, CustomOperation};

/// Provider for API Gateway custom operations
///
/// This provider loads CustomOperation and App resources from the database
/// and converts them to OperationDefinitions for the operations registry.
pub struct GatewayOperationProvider {
    operations: parking_lot::RwLock<Vec<OperationDefinition>>,
    storage: DynStorage,
}

impl GatewayOperationProvider {
    /// Create a new Gateway operations provider by loading from storage
    ///
    /// This is an async constructor that loads all active Apps and CustomOperations
    /// from the database and converts them to operation definitions.
    pub async fn new(storage: &DynStorage) -> Result<Self, Box<dyn std::error::Error>> {
        let operations = Self::load_operations(storage).await?;
        Ok(Self {
            operations: parking_lot::RwLock::new(operations),
            storage: storage.clone(),
        })
    }

    /// Reload operations from storage
    ///
    /// Called by GatewayReloadHook when App or CustomOperation resources change.
    /// This refreshes the in-memory cache with fresh data from the database.
    pub async fn reload(&self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Reloading Gateway operations from storage");
        let fresh_operations = Self::load_operations(&self.storage).await?;
        let mut ops = self.operations.write();
        *ops = fresh_operations;
        debug!(count = ops.len(), "Gateway operations reloaded");
        Ok(())
    }

    /// Load operations from storage
    async fn load_operations(
        storage: &DynStorage,
    ) -> Result<Vec<OperationDefinition>, Box<dyn std::error::Error>> {
        debug!("Loading Gateway custom operations from storage");

        // Load all active Apps
        let search_params = SearchParams::new().with_count(1000);
        let apps_result = storage.search("App", &search_params).await?;

        let apps: Vec<App> = apps_result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .filter(|app: &App| app.is_active())
            .collect();

        debug!(count = apps.len(), "Loaded active Apps");

        // Build a map of app ID -> App for quick lookup
        let app_map: HashMap<String, App> = apps
            .into_iter()
            .filter_map(|app| app.id.clone().map(|id| (id, app)))
            .collect();

        // Load all active CustomOperations
        let ops_result = storage.search("CustomOperation", &search_params).await?;

        let custom_operations: Vec<CustomOperation> = ops_result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .filter(|op: &CustomOperation| op.active)
            .collect();

        debug!(
            count = custom_operations.len(),
            "Loaded active CustomOperations"
        );

        // Convert CustomOperations to OperationDefinitions
        let mut operations = Vec::new();

        for custom_op in custom_operations {
            // Extract app reference
            let app_ref = match custom_op.app.reference.as_ref() {
                Some(r) => r,
                None => {
                    warn!(
                        operation_id = ?custom_op.id,
                        "Skipping CustomOperation with no app reference"
                    );
                    continue;
                }
            };

            // Extract app ID from reference (e.g., "App/123" -> "123")
            let app_id = match app_ref.split('/').next_back() {
                Some(id) => id,
                None => {
                    warn!(app_ref = %app_ref, "Invalid app reference format");
                    continue;
                }
            };

            // Find the app
            let app = match app_map.get(app_id) {
                Some(a) => a,
                None => {
                    warn!(app_id = %app_id, "App not found for CustomOperation");
                    continue;
                }
            };

            // Build full path by combining app base path and operation path
            let full_path = if let Some(base_path) = &app.base_path {
                format!("{}{}", base_path, custom_op.path)
            } else {
                custom_op.path.clone()
            };

            // Create operation ID from app name and operation path
            let operation_id = format!(
                "gateway.{}.{}",
                app.name.to_lowercase().replace(' ', "_"),
                custom_op
                    .id
                    .as_ref()
                    .unwrap_or(&"unknown".to_string())
                    .to_lowercase()
            );

            // Build description
            let description = format!(
                "Custom {} operation: {} (Type: {})",
                custom_op.method, full_path, custom_op.operation_type
            );

            // Create OperationDefinition with App reference
            let op_def = OperationDefinition::new(
                operation_id,
                format!(
                    "{} {}",
                    custom_op.method,
                    custom_op.id.as_deref().unwrap_or("Unknown")
                ),
                categories::API,
                vec![custom_op.method.clone()],
                full_path,
                app.id.clone().unwrap_or_else(|| "gateway".to_string()),
            )
            .with_description(description)
            .with_public(custom_op.public)
            .with_app(AppReference {
                id: app.id.clone().unwrap_or_default(),
                name: app.name.clone(),
            });

            operations.push(op_def);
        }

        debug!(
            count = operations.len(),
            "Converted CustomOperations to OperationDefinitions"
        );

        Ok(operations)
    }
}

impl OperationProvider for GatewayOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        self.operations.read().clone()
    }

    fn module_id(&self) -> &str {
        "gateway"
    }
}
