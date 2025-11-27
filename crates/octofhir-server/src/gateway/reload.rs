//! Hot-reload functionality for gateway routes.
//!
//! This module listens to PostgreSQL NOTIFY events for App and CustomOperation
//! resource changes and triggers automatic route reloading.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx_postgres::{PgListener, PgPool};
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

use super::GatewayRouter;
use octofhir_db_memory::DynStorage;

/// Channel for gateway resource change notifications.
const GATEWAY_CHANNEL: &str = "octofhir_gateway_changes";

/// Payload from PostgreSQL NOTIFY trigger.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NotifyPayload {
    table: String,
    resource_type: String,
    operation: String,
    id: String,
    version_id: i64,
}

/// Hot-reload listener for gateway routes.
///
/// Monitors PostgreSQL NOTIFY events for App and CustomOperation changes
/// and automatically reloads gateway routes.
pub struct GatewayReloadListener {
    pool: PgPool,
    gateway_router: Arc<GatewayRouter>,
    storage: DynStorage,
}

impl GatewayReloadListener {
    /// Creates a new gateway reload listener.
    pub fn new(pool: PgPool, gateway_router: Arc<GatewayRouter>, storage: DynStorage) -> Self {
        Self {
            pool,
            gateway_router,
            storage,
        }
    }

    /// Starts the hot-reload listener in the background.
    ///
    /// This spawns a tokio task that:
    /// 1. Connects to PostgreSQL with LISTEN
    /// 2. Receives NOTIFY events for App/CustomOperation changes
    /// 3. Triggers gateway route reload
    ///
    /// The task runs until an unrecoverable error occurs or the listener is dropped.
    ///
    /// # Returns
    ///
    /// A handle to the spawned task.
    #[instrument(skip(self))]
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        info!("Starting gateway hot-reload listener");

        tokio::spawn(async move {
            loop {
                match self.listen_loop().await {
                    Ok(()) => {
                        info!("Gateway hot-reload listener stopped gracefully");
                        break;
                    }
                    Err(e) => {
                        error!(error = %e, "Gateway hot-reload listener error, reconnecting in 5s");
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Main listen loop that connects and receives notifications.
    async fn listen_loop(&self) -> Result<(), Box<dyn std::error::Error + Send>> {
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        listener
            .listen(GATEWAY_CHANNEL)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        info!(
            channel = GATEWAY_CHANNEL,
            "Listening for gateway resource changes"
        );

        loop {
            let notification = listener
                .recv()
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            let payload = notification.payload();

            debug!(payload = %payload, "Received gateway NOTIFY");

            match serde_json::from_str::<NotifyPayload>(payload) {
                Ok(parsed) => {
                    info!(
                        resource_type = %parsed.resource_type,
                        operation = %parsed.operation,
                        id = %parsed.id,
                        "Gateway resource changed, reloading routes"
                    );

                    // Trigger route reload
                    match self.gateway_router.reload_routes(&self.storage).await {
                        Ok(count) => {
                            info!(count = count, "Gateway routes reloaded successfully");
                        }
                        Err(e) => {
                            error!(
                                error = %e,
                                "Failed to reload gateway routes"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        payload = %payload,
                        "Failed to parse gateway NOTIFY payload"
                    );
                }
            }
        }
    }
}

/// Builder for gateway hot-reload listener.
pub struct GatewayReloadBuilder {
    pool: Option<PgPool>,
    gateway_router: Option<Arc<GatewayRouter>>,
    storage: Option<DynStorage>,
}

impl GatewayReloadBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            pool: None,
            gateway_router: None,
            storage: None,
        }
    }

    /// Sets the PostgreSQL pool.
    pub fn with_pool(mut self, pool: PgPool) -> Self {
        self.pool = Some(pool);
        self
    }

    /// Sets the gateway router.
    pub fn with_gateway_router(mut self, router: Arc<GatewayRouter>) -> Self {
        self.gateway_router = Some(router);
        self
    }

    /// Sets the storage.
    pub fn with_storage(mut self, storage: DynStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Starts the hot-reload listener.
    ///
    /// Returns a handle to the spawned task.
    pub fn start(self) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error>> {
        let pool = self.pool.ok_or("PostgreSQL pool is required")?;
        let gateway_router = self.gateway_router.ok_or("Gateway router is required")?;
        let storage = self.storage.ok_or("Storage is required")?;

        let listener = Arc::new(GatewayReloadListener::new(pool, gateway_router, storage));
        Ok(listener.start())
    }
}

impl Default for GatewayReloadBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        let builder = GatewayReloadBuilder::new();
        assert!(builder.pool.is_none());
        assert!(builder.gateway_router.is_none());
        assert!(builder.storage.is_none());
    }
}
