//! FHIR resource read caching for performance optimization.
//!
//! Caches raw FHIR resources in-process to avoid database round-trips for reads.
//! Backed by a bounded `moka` cache (size-capped + TTL) keyed by
//! `res:{resource_type}:{id}`. Entries are stored as `Arc<RawStoredResource>`,
//! so a hit is a refcount bump with no deserialization.
//!
//! Invalidated on create, update, and delete via [`ResourceCache::invalidate`].

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use moka::future::Cache;

use octofhir_storage::RawStoredResource;

/// FHIR resource read cache.
pub struct ResourceCache {
    cache: Cache<String, CachedResource>,
    generation: AtomicU64,
    // Bounded stripes avoid an unbounded map of invalidated identities. A
    // collision only causes an extra cache miss; it cannot retain stale JSON.
    stripes: [AtomicU64; 4096],
    enabled: AtomicBool,
}

/// Opaque generation captured before a database fetch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FillStamp {
    stripe: usize,
    generation: u64,
    revision: u64,
}

#[derive(Clone)]
struct CachedResource {
    resource: Arc<RawStoredResource>,
    stamp: FillStamp,
}

impl ResourceCache {
    /// Create a new resource cache bounded to `max_capacity` entries with the
    /// given per-entry TTL.
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .build(),
            generation: AtomicU64::new(0),
            stripes: std::array::from_fn(|_| AtomicU64::new(0)),
            enabled: AtomicBool::new(true),
        }
    }

    #[inline]
    fn cache_key(resource_type: &str, id: &str) -> String {
        format!("res:{resource_type}:{id}")
    }

    /// Capture before a database read. Invalidations during that read make
    /// its eventual cache insertion unusable, even if insertion wins the race.
    pub fn begin_fill(&self, resource_type: &str, id: &str) -> FillStamp {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        resource_type.hash(&mut hasher);
        id.hash(&mut hasher);
        let stripe = hasher.finish() as usize % self.stripes.len();
        FillStamp {
            stripe,
            generation: self.generation.load(Ordering::SeqCst),
            revision: self.stripes[stripe].load(Ordering::SeqCst),
        }
    }

    fn is_current(&self, stamp: FillStamp) -> bool {
        self.enabled.load(Ordering::SeqCst)
            && self.generation.load(Ordering::SeqCst) == stamp.generation
            && self.stripes[stamp.stripe].load(Ordering::SeqCst) == stamp.revision
    }

    /// Get a cached resource by type and ID.
    pub async fn get(&self, resource_type: &str, id: &str) -> Option<Arc<RawStoredResource>> {
        let key = Self::cache_key(resource_type, id);
        let cached = self.cache.get(&key).await?;
        self.is_current(cached.stamp).then_some(cached.resource)
    }

    /// Cache a successful read using the stamp captured before fetching it.
    pub async fn set(&self, stored: &RawStoredResource, stamp: FillStamp) {
        if !self.is_current(stamp) {
            return;
        }
        let key = Self::cache_key(&stored.resource_type, &stored.id);
        self.cache
            .insert(
                key,
                CachedResource {
                    resource: Arc::new(stored.clone()),
                    stamp,
                },
            )
            .await;
    }

    /// Invalidate a cached resource and any in-flight reads of its identity.
    pub async fn invalidate(&self, resource_type: &str, id: &str) {
        let stamp = self.begin_fill(resource_type, id);
        self.stripes[stamp.stripe].fetch_add(1, Ordering::SeqCst);
        let key = Self::cache_key(resource_type, id);
        self.cache.invalidate(&key).await;
    }

    /// Discard everything after a lost event or a synchronization gap.
    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.cache.invalidate_all();
    }

    /// Suspend caching while cross-replica synchronization is unavailable.
    pub fn suspend(&self) {
        self.enabled.store(false, Ordering::SeqCst);
        self.clear();
    }

    /// Resume only after subscribing, discarding reads begun during the gap.
    pub fn resume(&self) {
        self.clear();
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Invalidate for committed local and imported events. A lagged receiver
    /// cannot know which identities changed, so it discards the entire cache.
    pub async fn listen(
        &self,
        mut receiver: tokio::sync::broadcast::Receiver<octofhir_core::events::SystemEvent>,
    ) {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match receiver.recv().await {
                Ok(octofhir_core::events::SystemEvent::Resource(event)) => {
                    self.invalidate(&event.resource_type, &event.resource_id)
                        .await;
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => self.clear(),
                Err(RecvError::Closed) => {
                    self.suspend();
                    return;
                }
            }
        }
    }

    /// Current number of cached entries.
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::events::{EventBroadcaster, ResourceEvent};

    fn resource(id: &str, version: &str) -> RawStoredResource {
        RawStoredResource {
            id: id.into(),
            version_id: version.into(),
            resource_type: "Patient".into(),
            resource_json:
                serde_json::json!({"resourceType":"Patient","id":id,"meta":{"versionId":version}})
                    .to_string(),
            last_updated: time::OffsetDateTime::now_utc(),
            created_at: time::OffsetDateTime::now_utc(),
        }
    }

    #[tokio::test]
    async fn invalidation_rejects_reads_that_finish_after_a_write() {
        let cache = ResourceCache::new(100, Duration::from_secs(60));
        let old = resource("race", "1");
        let in_flight = cache.begin_fill("Patient", "race");
        cache.invalidate("Patient", "race").await;
        let new = resource("race", "2");
        cache.set(&new, cache.begin_fill("Patient", "race")).await;
        // The earlier database read completes after the update invalidated it.
        cache.set(&old, in_flight).await;
        assert_eq!(cache.get("Patient", "race").await.unwrap().version_id, "2");
        // Even an insertion that passed the pre-check before invalidation must
        // be rejected on lookup, rather than resurrecting the previous JSON.
        cache
            .cache
            .insert(
                ResourceCache::cache_key("Patient", "race"),
                CachedResource {
                    resource: Arc::new(old),
                    stamp: in_flight,
                },
            )
            .await;
        assert!(cache.get("Patient", "race").await.is_none());
    }

    #[tokio::test]
    async fn synchronization_gaps_discard_hits_and_in_flight_fills() {
        let cache = ResourceCache::new(100, Duration::from_secs(60));
        let stored = resource("gap", "1");
        let before = cache.begin_fill("Patient", "gap");
        cache.set(&stored, before).await;
        cache.suspend();
        assert!(cache.get("Patient", "gap").await.is_none());
        let during = cache.begin_fill("Patient", "gap");
        cache.set(&stored, during).await;
        assert!(cache.get("Patient", "gap").await.is_none());
        cache.resume();
        cache.set(&stored, before).await;
        cache.set(&stored, during).await;
        assert!(cache.get("Patient", "gap").await.is_none());
        cache.set(&stored, cache.begin_fill("Patient", "gap")).await;
        assert!(cache.get("Patient", "gap").await.is_some());
    }

    #[tokio::test]
    async fn imported_events_and_receiver_lag_invalidate_resources() {
        let cache = Arc::new(ResourceCache::new(100, Duration::from_secs(60)));
        let broadcaster = EventBroadcaster::with_capacity(1);
        let stored = resource("missed", "1");
        cache
            .set(&stored, cache.begin_fill("Patient", "missed"))
            .await;
        let receiver = broadcaster.subscribe();
        // Overflow before the listener can consume: the event for our cached
        // identity is lost, so processing only the retained identity is unsafe.
        broadcaster.send_deleted("Patient", "missed");
        let mut imported = ResourceEvent::deleted("Observation", "retained");
        imported.remote = true;
        broadcaster.send_resource(imported);
        let task = {
            let cache = cache.clone();
            tokio::spawn(async move { cache.listen(receiver).await })
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            while cache.get("Patient", "missed").await.is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cache
            .set(&stored, cache.begin_fill("Patient", "missed"))
            .await;
        let mut imported = ResourceEvent::deleted("Patient", "missed");
        imported.remote = true;
        broadcaster.send_resource(imported);
        tokio::time::timeout(Duration::from_secs(2), async {
            while cache.get("Patient", "missed").await.is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(broadcaster);
        task.await.unwrap();
        cache
            .set(&stored, cache.begin_fill("Patient", "missed"))
            .await;
        assert!(cache.get("Patient", "missed").await.is_none());
    }
}
