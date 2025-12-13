//! Policy hot-reload functionality.
//!
//! This module provides hot-reload support for policies, enabling policy changes
//! to take effect without server restart.
//!
//! # Architecture
//!
//! The hot-reload system consists of:
//!
//! - [`PolicyChange`] - Events representing policy changes
//! - [`PolicyChangeNotifier`] - Broadcast channel for change notifications
//! - [`PolicyReloadService`] - Service that handles reloading with debouncing and retry
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::policy::reload::{
//!     PolicyChangeNotifier, PolicyReloadService, ReloadConfig,
//! };
//! use std::sync::Arc;
//!
//! // Create the notification channel
//! let notifier = Arc::new(PolicyChangeNotifier::new(64));
//!
//! // Create the reload service
//! let service = Arc::new(PolicyReloadService::new(
//!     cache,
//!     notifier.clone(),
//!     ReloadConfig::default(),
//! ));
//!
//! // Start the service (runs in background)
//! let handle = tokio::spawn({
//!     let service = service.clone();
//!     async move { service.run().await }
//! });
//!
//! // Notify of a policy change
//! notifier.notify(PolicyChange::Updated { policy_id: "policy-1".to_string() });
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::broadcast;
use tokio::time::{Duration, Instant};

use crate::AuthError;
use crate::policy::cache::PolicyCache;

// =============================================================================
// Policy Change Types
// =============================================================================

/// Types of policy changes that can trigger a reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyChange {
    /// A new policy was created.
    Created {
        /// The ID of the created policy.
        policy_id: String,
    },
    /// An existing policy was updated.
    Updated {
        /// The ID of the updated policy.
        policy_id: String,
    },
    /// A policy was deleted.
    Deleted {
        /// The ID of the deleted policy.
        policy_id: String,
    },
    /// Request to reload all policies.
    BulkReload,
}

impl PolicyChange {
    /// Get the policy ID if this is a single-policy change.
    #[must_use]
    pub fn policy_id(&self) -> Option<&str> {
        match self {
            Self::Created { policy_id }
            | Self::Updated { policy_id }
            | Self::Deleted { policy_id } => Some(policy_id),
            Self::BulkReload => None,
        }
    }

    /// Check if this is a bulk reload request.
    #[must_use]
    pub fn is_bulk_reload(&self) -> bool {
        matches!(self, Self::BulkReload)
    }
}

// =============================================================================
// Policy Change Notifier
// =============================================================================

/// Broadcast channel for policy change notifications.
///
/// Multiple producers can send notifications, and multiple consumers can
/// subscribe to receive them.
pub struct PolicyChangeNotifier {
    sender: broadcast::Sender<PolicyChange>,
}

impl PolicyChangeNotifier {
    /// Create a new policy change notifier.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of pending notifications in the channel
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Notify all subscribers of a policy change.
    ///
    /// If there are no subscribers, the notification is silently dropped.
    pub fn notify(&self, change: PolicyChange) {
        // Ignore send errors (no receivers)
        let _ = self.sender.send(change);
    }

    /// Subscribe to policy change notifications.
    ///
    /// Returns a receiver that will receive all future notifications.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<PolicyChange> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for PolicyChangeNotifier {
    fn default() -> Self {
        Self::new(64)
    }
}

// =============================================================================
// Reload Configuration
// =============================================================================

/// Configuration for the policy reload service.
#[derive(Debug, Clone)]
pub struct ReloadConfig {
    /// Debounce time in milliseconds.
    ///
    /// Multiple rapid changes will be batched into a single reload.
    pub debounce_ms: u64,

    /// Periodic refresh interval in seconds.
    ///
    /// Set to 0 to disable periodic refresh.
    pub periodic_refresh_secs: u64,

    /// Maximum number of retry attempts on failure.
    pub max_retry_attempts: usize,

    /// Initial retry backoff in milliseconds.
    ///
    /// Backoff doubles on each retry.
    pub retry_backoff_ms: u64,
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 100,
            periodic_refresh_secs: 300, // 5 minutes
            max_retry_attempts: 3,
            retry_backoff_ms: 1000,
        }
    }
}

impl ReloadConfig {
    /// Create a configuration with no periodic refresh.
    #[must_use]
    pub fn without_periodic_refresh() -> Self {
        Self {
            periodic_refresh_secs: 0,
            ..Default::default()
        }
    }

    /// Create a configuration for testing with fast timeouts.
    #[must_use]
    pub fn for_testing() -> Self {
        Self {
            debounce_ms: 10,
            periodic_refresh_secs: 0,
            max_retry_attempts: 3,
            retry_backoff_ms: 10,
        }
    }
}

// =============================================================================
// Reload Statistics
// =============================================================================

/// Statistics about the reload service.
#[derive(Debug, Clone, Default)]
pub struct ReloadStats {
    /// Total number of reload attempts.
    pub reload_attempts: u64,
    /// Number of successful reloads.
    pub successful_reloads: u64,
    /// Number of failed reloads.
    pub failed_reloads: u64,
    /// Number of notifications received.
    pub notifications_received: u64,
    /// Number of notifications skipped due to debouncing.
    pub notifications_debounced: u64,
}

// =============================================================================
// Policy Reload Service
// =============================================================================

/// Service that handles policy reloading with debouncing and retry logic.
///
/// The service:
/// - Listens for policy change notifications
/// - Debounces rapid changes to avoid excessive reloads
/// - Periodically refreshes policies (optional)
/// - Retries failed reloads with exponential backoff
pub struct PolicyReloadService {
    /// The policy cache to refresh.
    policy_cache: Arc<PolicyCache>,

    /// Notification channel for policy changes.
    notifier: Arc<PolicyChangeNotifier>,

    /// Configuration.
    config: ReloadConfig,

    /// Flag to signal shutdown.
    shutdown: AtomicBool,

    /// Statistics counters.
    reload_attempts: AtomicU64,
    successful_reloads: AtomicU64,
    failed_reloads: AtomicU64,
    notifications_received: AtomicU64,
    notifications_debounced: AtomicU64,
}

impl PolicyReloadService {
    /// Create a new policy reload service.
    #[must_use]
    pub fn new(
        policy_cache: Arc<PolicyCache>,
        notifier: Arc<PolicyChangeNotifier>,
        config: ReloadConfig,
    ) -> Self {
        Self {
            policy_cache,
            notifier,
            config,
            shutdown: AtomicBool::new(false),
            reload_attempts: AtomicU64::new(0),
            successful_reloads: AtomicU64::new(0),
            failed_reloads: AtomicU64::new(0),
            notifications_received: AtomicU64::new(0),
            notifications_debounced: AtomicU64::new(0),
        }
    }

    /// Run the reload service.
    ///
    /// This method runs until `shutdown()` is called. It:
    /// - Listens for policy change notifications
    /// - Periodically refreshes policies (if configured)
    /// - Handles debouncing and retries
    pub async fn run(&self) {
        let mut receiver = self.notifier.subscribe();
        let debounce_duration = Duration::from_millis(self.config.debounce_ms);
        let periodic_duration = if self.config.periodic_refresh_secs > 0 {
            Some(Duration::from_secs(self.config.periodic_refresh_secs))
        } else {
            None
        };

        let mut pending_reload = false;
        let mut last_notification = Instant::now();
        let mut last_periodic_refresh = Instant::now();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!("Policy reload service shutting down");
                break;
            }

            // Calculate next timeout
            let debounce_remaining = if pending_reload {
                debounce_duration.saturating_sub(last_notification.elapsed())
            } else {
                debounce_duration
            };

            let periodic_remaining = periodic_duration
                .map(|d| d.saturating_sub(last_periodic_refresh.elapsed()))
                .unwrap_or(Duration::MAX);

            let timeout = if pending_reload {
                debounce_remaining.min(periodic_remaining)
            } else {
                periodic_remaining
            };

            tokio::select! {
                // Handle incoming notifications
                result = receiver.recv() => {
                    match result {
                        Ok(change) => {
                            tracing::debug!(change = ?change, "Policy change received");
                            self.notifications_received.fetch_add(1, Ordering::Relaxed);

                            if pending_reload {
                                // Already have a pending reload, this is debounced
                                self.notifications_debounced.fetch_add(1, Ordering::Relaxed);
                            }

                            pending_reload = true;
                            last_notification = Instant::now();
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(missed = n, "Missed policy change notifications");
                            self.notifications_debounced.fetch_add(n, Ordering::Relaxed);
                            pending_reload = true;
                            last_notification = Instant::now();
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::info!("Policy change channel closed");
                            break;
                        }
                    }
                }

                // Handle timeout (debounce or periodic)
                _ = tokio::time::sleep(timeout) => {
                    if pending_reload && last_notification.elapsed() >= debounce_duration {
                        // Debounce period elapsed, perform reload
                        pending_reload = false;
                        self.perform_reload().await;
                        last_periodic_refresh = Instant::now();
                    } else if periodic_duration.is_some()
                        && last_periodic_refresh.elapsed() >= periodic_duration.unwrap()
                    {
                        // Periodic refresh time
                        tracing::debug!("Periodic policy refresh");
                        self.perform_reload().await;
                        last_periodic_refresh = Instant::now();
                    }
                }
            }
        }
    }

    /// Perform a reload with retry logic.
    async fn perform_reload(&self) {
        if let Err(e) = self.reload_with_retry().await {
            tracing::error!(error = %e, "Policy reload failed after all retries");
        }
    }

    /// Reload policies with retry and exponential backoff.
    pub async fn reload_with_retry(&self) -> Result<(), AuthError> {
        let mut attempts = 0;
        let mut backoff = self.config.retry_backoff_ms;

        loop {
            self.reload_attempts.fetch_add(1, Ordering::Relaxed);

            match self.policy_cache.refresh().await {
                Ok(()) => {
                    self.successful_reloads.fetch_add(1, Ordering::Relaxed);
                    let version = self.policy_cache.version().await;
                    tracing::info!(version = version, "Policy cache reloaded successfully");
                    return Ok(());
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.config.max_retry_attempts {
                        self.failed_reloads.fetch_add(1, Ordering::Relaxed);
                        return Err(e);
                    }

                    tracing::warn!(
                        attempt = attempts,
                        max_attempts = self.config.max_retry_attempts,
                        error = %e,
                        backoff_ms = backoff,
                        "Policy reload failed, retrying"
                    );

                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    backoff *= 2; // Exponential backoff
                }
            }
        }
    }

    /// Trigger an immediate reload.
    ///
    /// Sends a `BulkReload` notification to the channel.
    pub fn trigger_reload(&self) {
        self.notifier.notify(PolicyChange::BulkReload);
    }

    /// Signal the service to shut down.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Check if the service is shutting down.
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    /// Get reload statistics.
    #[must_use]
    pub fn stats(&self) -> ReloadStats {
        ReloadStats {
            reload_attempts: self.reload_attempts.load(Ordering::Relaxed),
            successful_reloads: self.successful_reloads.load(Ordering::Relaxed),
            failed_reloads: self.failed_reloads.load(Ordering::Relaxed),
            notifications_received: self.notifications_received.load(Ordering::Relaxed),
            notifications_debounced: self.notifications_debounced.load(Ordering::Relaxed),
        }
    }

    /// Get the underlying policy cache.
    #[must_use]
    pub fn policy_cache(&self) -> &Arc<PolicyCache> {
        &self.policy_cache
    }

    /// Get the underlying notifier.
    #[must_use]
    pub fn notifier(&self) -> &Arc<PolicyChangeNotifier> {
        &self.notifier
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthResult;
    use crate::policy::resources::{AccessPolicy, EngineElement, MatcherElement, PolicyEngineType};
    use crate::storage::PolicyStorage;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use time::Duration as TimeDuration;

    // -------------------------------------------------------------------------
    // Mock Storage
    // -------------------------------------------------------------------------

    struct MockPolicyStorage {
        policies: std::sync::RwLock<Vec<AccessPolicy>>,
        call_count: AtomicUsize,
        fail_count: AtomicUsize,
    }

    impl MockPolicyStorage {
        fn new() -> Self {
            Self {
                policies: std::sync::RwLock::new(Vec::new()),
                call_count: AtomicUsize::new(0),
                fail_count: AtomicUsize::new(0),
            }
        }

        fn with_policies(policies: Vec<AccessPolicy>) -> Self {
            Self {
                policies: std::sync::RwLock::new(policies),
                call_count: AtomicUsize::new(0),
                fail_count: AtomicUsize::new(0),
            }
        }

        fn set_fail_count(&self, count: usize) {
            self.fail_count.store(count, Ordering::SeqCst);
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl PolicyStorage for MockPolicyStorage {
        async fn get(&self, id: &str) -> AuthResult<Option<AccessPolicy>> {
            Ok(self
                .policies
                .read()
                .unwrap()
                .iter()
                .find(|p| p.id.as_deref() == Some(id))
                .cloned())
        }

        async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            let fail_count = self.fail_count.load(Ordering::SeqCst);

            if count < fail_count {
                return Err(AuthError::storage("Simulated storage failure"));
            }

            Ok(self
                .policies
                .read()
                .unwrap()
                .iter()
                .filter(|p| p.active)
                .cloned()
                .collect())
        }

        async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>> {
            Ok(self.policies.read().unwrap().clone())
        }

        async fn create(&self, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn update(&self, _id: &str, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn delete(&self, _id: &str) -> AuthResult<()> {
            unimplemented!()
        }

        async fn find_applicable(
            &self,
            _resource_type: &str,
            _operation: crate::smart::scopes::FhirOperation,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn get_by_ids(&self, _ids: &[String]) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn search(
            &self,
            _params: &crate::storage::PolicySearchParams,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_client(&self, _client_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_user(&self, _user_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_role(&self, _role: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn upsert(&self, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }
    }

    // -------------------------------------------------------------------------
    // Helper Functions
    // -------------------------------------------------------------------------

    fn create_policy(id: &str, priority: i32, resource_types: Option<Vec<&str>>) -> AccessPolicy {
        AccessPolicy {
            id: Some(id.to_string()),
            name: format!("Policy {}", id),
            priority,
            active: true,
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                resource_types: resource_types.map(|v| v.into_iter().map(String::from).collect()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // -------------------------------------------------------------------------
    // PolicyChange Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_policy_change_policy_id() {
        let change = PolicyChange::Created {
            policy_id: "p1".to_string(),
        };
        assert_eq!(change.policy_id(), Some("p1"));

        let change = PolicyChange::Updated {
            policy_id: "p2".to_string(),
        };
        assert_eq!(change.policy_id(), Some("p2"));

        let change = PolicyChange::Deleted {
            policy_id: "p3".to_string(),
        };
        assert_eq!(change.policy_id(), Some("p3"));

        let change = PolicyChange::BulkReload;
        assert_eq!(change.policy_id(), None);
    }

    #[test]
    fn test_policy_change_is_bulk_reload() {
        assert!(
            !PolicyChange::Created {
                policy_id: "p1".to_string()
            }
            .is_bulk_reload()
        );
        assert!(PolicyChange::BulkReload.is_bulk_reload());
    }

    // -------------------------------------------------------------------------
    // PolicyChangeNotifier Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_notifier_subscribe_and_receive() {
        let notifier = PolicyChangeNotifier::new(16);
        let mut receiver = notifier.subscribe();

        notifier.notify(PolicyChange::Created {
            policy_id: "p1".to_string(),
        });

        let change = receiver.recv().await.unwrap();
        assert!(matches!(
            change,
            PolicyChange::Created { policy_id } if policy_id == "p1"
        ));
    }

    #[tokio::test]
    async fn test_notifier_multiple_subscribers() {
        let notifier = PolicyChangeNotifier::new(16);
        let mut receiver1 = notifier.subscribe();
        let mut receiver2 = notifier.subscribe();

        assert_eq!(notifier.subscriber_count(), 2);

        notifier.notify(PolicyChange::Updated {
            policy_id: "p1".to_string(),
        });

        let change1 = receiver1.recv().await.unwrap();
        let change2 = receiver2.recv().await.unwrap();

        assert_eq!(change1, change2);
    }

    #[tokio::test]
    async fn test_notifier_no_subscribers() {
        let notifier = PolicyChangeNotifier::new(16);

        // Should not panic when no subscribers
        notifier.notify(PolicyChange::Deleted {
            policy_id: "p1".to_string(),
        });
    }

    #[test]
    fn test_notifier_default() {
        let notifier = PolicyChangeNotifier::default();
        assert_eq!(notifier.subscriber_count(), 0);
    }

    // -------------------------------------------------------------------------
    // ReloadConfig Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_reload_config_default() {
        let config = ReloadConfig::default();
        assert_eq!(config.debounce_ms, 100);
        assert_eq!(config.periodic_refresh_secs, 300);
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.retry_backoff_ms, 1000);
    }

    #[test]
    fn test_reload_config_without_periodic() {
        let config = ReloadConfig::without_periodic_refresh();
        assert_eq!(config.periodic_refresh_secs, 0);
    }

    #[test]
    fn test_reload_config_for_testing() {
        let config = ReloadConfig::for_testing();
        assert_eq!(config.debounce_ms, 10);
        assert_eq!(config.periodic_refresh_secs, 0);
        assert_eq!(config.retry_backoff_ms, 10);
    }

    // -------------------------------------------------------------------------
    // PolicyReloadService Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_reload_with_retry_success() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::for_testing();

        let service = PolicyReloadService::new(cache.clone(), notifier, config);

        let result = service.reload_with_retry().await;
        assert!(result.is_ok());
        assert_eq!(cache.version().await, 1);
        assert_eq!(storage.call_count(), 1);

        let stats = service.stats();
        assert_eq!(stats.reload_attempts, 1);
        assert_eq!(stats.successful_reloads, 1);
        assert_eq!(stats.failed_reloads, 0);
    }

    #[tokio::test]
    async fn test_reload_with_retry_recovers() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));
        storage.set_fail_count(2); // Fail first 2 attempts

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::for_testing();

        let service = PolicyReloadService::new(cache.clone(), notifier, config);

        let result = service.reload_with_retry().await;
        assert!(result.is_ok());
        assert_eq!(storage.call_count(), 3); // 2 failures + 1 success

        let stats = service.stats();
        assert_eq!(stats.reload_attempts, 3);
        assert_eq!(stats.successful_reloads, 1);
        assert_eq!(stats.failed_reloads, 0);
    }

    #[tokio::test]
    async fn test_reload_with_retry_exhausted() {
        let storage = Arc::new(MockPolicyStorage::new());
        storage.set_fail_count(10); // Always fail

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::for_testing();

        let service = PolicyReloadService::new(cache.clone(), notifier, config);

        let result = service.reload_with_retry().await;
        assert!(result.is_err());
        assert_eq!(storage.call_count(), 3); // max_retry_attempts

        let stats = service.stats();
        assert_eq!(stats.reload_attempts, 3);
        assert_eq!(stats.successful_reloads, 0);
        assert_eq!(stats.failed_reloads, 1);
    }

    #[tokio::test]
    async fn test_trigger_reload() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let mut receiver = notifier.subscribe();

        let config = ReloadConfig::for_testing();
        let service = PolicyReloadService::new(cache, notifier.clone(), config);

        service.trigger_reload();

        let change = receiver.recv().await.unwrap();
        assert!(matches!(change, PolicyChange::BulkReload));
    }

    #[tokio::test]
    async fn test_shutdown() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::for_testing();

        let service = Arc::new(PolicyReloadService::new(cache, notifier, config));

        assert!(!service.is_shutting_down());

        service.shutdown();

        assert!(service.is_shutting_down());
    }

    #[tokio::test]
    async fn test_service_run_with_notification() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::for_testing();

        let service = Arc::new(PolicyReloadService::new(
            cache.clone(),
            notifier.clone(),
            config,
        ));

        // Start service in background
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            service_clone.run().await;
        });

        // Give service time to start
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Send notification
        notifier.notify(PolicyChange::Updated {
            policy_id: "p1".to_string(),
        });

        // Wait for debounce + processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check reload happened
        assert!(cache.version().await >= 1);

        // Shutdown
        service.shutdown();
        let _ = tokio::time::timeout(Duration::from_millis(100), handle).await;
    }

    #[tokio::test]
    async fn test_service_debouncing() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        let cache = Arc::new(PolicyCache::new(storage.clone(), TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig {
            debounce_ms: 50,
            periodic_refresh_secs: 0,
            max_retry_attempts: 1,
            retry_backoff_ms: 10,
        };

        let service = Arc::new(PolicyReloadService::new(
            cache.clone(),
            notifier.clone(),
            config,
        ));

        // Start service
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            service_clone.run().await;
        });

        // Give service time to start
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Send multiple rapid notifications
        for i in 0..5 {
            notifier.notify(PolicyChange::Updated {
                policy_id: format!("p{}", i),
            });
        }

        // Wait for debounce + processing
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should have received all 5 notifications but only reloaded once due to debouncing
        let stats = service.stats();
        assert_eq!(stats.notifications_received, 5);
        assert!(stats.notifications_debounced >= 4); // At least 4 were debounced

        // Shutdown
        service.shutdown();
        let _ = tokio::time::timeout(Duration::from_millis(100), handle).await;
    }

    #[tokio::test]
    async fn test_accessors() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = Arc::new(PolicyCache::new(storage, TimeDuration::minutes(5)));
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let config = ReloadConfig::default();

        let service = PolicyReloadService::new(cache.clone(), notifier.clone(), config);

        // Test accessors
        assert!(Arc::ptr_eq(service.policy_cache(), &cache));
        assert!(Arc::ptr_eq(service.notifier(), &notifier));
    }
}
