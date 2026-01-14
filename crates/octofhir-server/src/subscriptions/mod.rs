//! FHIR R5 Topic-Based Subscriptions
//!
//! This module implements FHIR R5-style topic-based subscriptions with support for:
//! - R5 native `SubscriptionTopic` and `Subscription` resources
//! - R4/R4B servers via the [Subscriptions Backport IG](https://build.fhir.org/ig/HL7/fhir-subscription-backport-ig/)
//!
//! ## Architecture
//!
//! ```text
//! FHIR Write (POST/PUT/DELETE)
//!        ↓
//! EventBroadcaster → SubscriptionHook
//!        ↓
//! TopicRegistry (find matching topics)
//!        ↓
//! SubscriptionManager (find active subscriptions)
//!        ↓
//! EventMatcher (evaluate FHIRPath filters)
//!        ↓
//! subscription_event table (queue)
//!        ↓
//! DeliveryProcessor (background worker)
//!        ↓
//! Channels (REST-hook, WebSocket, Email)
//! ```
//!
//! ## Configuration
//!
//! For R4 servers, ensure the Subscriptions Backport IG is loaded:
//!
//! ```toml
//! [packages]
//! load = [
//!   "hl7.fhir.r4.core#4.0.1",
//!   "hl7.fhir.uv.subscriptions-backport#1.1.0"
//! ]
//! ```
//!
//! ## Channels
//!
//! Supported notification channels:
//! - `rest-hook` - HTTP POST to webhook endpoint
//! - `websocket` - WebSocket connection at `/fhir/Subscription/{id}/$events`
//! - `email` - Email notifications (via configured SMTP)

pub mod delivery;
pub mod error;
pub mod event_matcher;
pub mod handlers;
pub mod hook;
pub mod operations;
pub mod storage;
pub mod subscription_manager;
pub mod topic_registry;
pub mod types;

pub use delivery::{WebSocketRegistry, handle_subscription_websocket};
pub use error::{SubscriptionError, SubscriptionResult};
pub use event_matcher::EventMatcher;
pub use hook::SubscriptionHook;
pub use storage::SubscriptionEventStorage;
pub use subscription_manager::SubscriptionManager;
pub use topic_registry::TopicRegistry;
pub use types::*;

use std::sync::Arc;

use crate::server::AppState;

/// Subscription service state containing all subscription-related components.
#[derive(Clone)]
pub struct SubscriptionState {
    /// Topic registry for efficient topic lookup
    pub topic_registry: Arc<TopicRegistry>,

    /// Subscription manager for subscription lifecycle
    pub subscription_manager: Arc<SubscriptionManager>,

    /// Event matcher for FHIRPath filter evaluation
    pub event_matcher: Arc<EventMatcher>,

    /// Event storage for queue operations
    pub event_storage: Arc<SubscriptionEventStorage>,

    /// WebSocket connection registry
    pub websocket_registry: Arc<WebSocketRegistry>,

    /// Whether subscriptions are enabled
    pub enabled: bool,

    /// Shutdown handle for delivery processor (kept alive for server lifetime)
    pub delivery_shutdown: Option<Arc<tokio::sync::watch::Sender<bool>>>,
}

impl SubscriptionState {
    /// Create subscription state from application state.
    pub async fn from_app_state(app_state: &AppState) -> SubscriptionResult<Self> {
        let db_pool = (*app_state.db_pool).clone();
        let storage = app_state.storage.clone();
        let fhirpath_engine = app_state.fhirpath_engine.clone();

        // Create event storage
        let event_storage = Arc::new(SubscriptionEventStorage::new(db_pool.clone()));

        // Create topic registry
        let topic_registry = Arc::new(TopicRegistry::new(storage.clone()));

        // Create subscription manager
        let subscription_manager =
            Arc::new(SubscriptionManager::new(storage.clone(), db_pool.clone()));

        // Create event matcher with FHIRPath engine
        let event_matcher = Arc::new(EventMatcher::new(fhirpath_engine));

        // Create WebSocket registry
        let websocket_registry = Arc::new(WebSocketRegistry::new());

        Ok(Self {
            topic_registry,
            subscription_manager,
            event_matcher,
            event_storage,
            websocket_registry,
            enabled: true,           // TODO: Make configurable
            delivery_shutdown: None, // Set by server.rs when delivery processor starts
        })
    }

    /// Create the subscription hook for registering with the event system.
    pub fn create_hook(&self) -> SubscriptionHook {
        SubscriptionHook::new(
            self.topic_registry.clone(),
            self.subscription_manager.clone(),
            self.event_matcher.clone(),
            self.event_storage.clone(),
            self.enabled,
        )
    }
}

/// Initialize subscription subsystem.
///
/// This should be called during server startup to:
/// 1. Load subscription topics into cache
/// 2. Start the delivery processor
/// 3. Register the subscription hook
pub async fn init_subscriptions(app_state: &AppState) -> SubscriptionResult<SubscriptionState> {
    tracing::info!("Initializing subscription subsystem");

    let state = SubscriptionState::from_app_state(app_state).await?;

    // Load topics into cache
    state.topic_registry.reload().await?;

    tracing::info!(
        topics = state.topic_registry.topic_count(),
        "Subscription subsystem initialized"
    );

    Ok(state)
}
