//! Two-tier caching system for horizontal scaling.
//!
//! ## Architecture
//!
//! - **L1 Cache (DashMap)**: In-memory, microsecond latency, per-instance
//! - **L2 Cache (Redis)**: Network, millisecond latency, shared across instances
//! - **Pub/Sub**: Cross-instance cache invalidation
//!
//! ## Cache Hierarchy
//!
//! ```text
//! GET request → L1 (DashMap) → L2 (Redis) → Source (DB/API)
//!                   ↓                ↓            ↓
//!               <1µs latency    ~5ms latency  ~50ms latency
//! ```
//!
//! ## Graceful Degradation
//!
//! If Redis is unavailable or disabled, the system automatically falls back
//! to L1-only mode (local cache per instance).

pub mod backend;
pub mod pubsub;

pub use backend::{CacheBackend, CachedEntry};
pub use pubsub::{CacheInvalidationListener, publish_invalidation};
