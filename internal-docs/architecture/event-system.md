# Unified Event System Architecture

## Overview

The unified event system replaces multiple PostgreSQL LISTEN/NOTIFY listeners with a centralized application-level event bus. This provides:

- **Inter-module communication** - Modules communicate through events without direct dependencies
- **Server as orchestrator** - `octofhir-server` connects modules via event bus
- **Async hooks** - Non-blocking cache invalidation and reloads
- **Redis pub/sub** - Multi-instance synchronization
- **GraphQL subscriptions** - Real-time updates to clients
- **Resilience** - Hook failures are isolated, don't affect other hooks or API

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Application Layer                           │
├─────────────────────────────────────────────────────────────────────┤
│  REST Handler │ GraphQL │ Transaction  →  Storage Layer (CRUD)      │
│       │              │          │                  │                │
│       └──────────────┴──────────┴──────────────────┘                │
│                              │                                       │
│                     emit ResourceEvent                               │
│                              ↓                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │              EventBroadcaster (tokio::broadcast)               │  │
│  │                      (central event bus)                       │  │
│  └───────────────────────────────────────────────────────────────┘  │
│         │              │              │              │               │
│         ↓              ↓              ↓              ↓               │
│    ┌────────┐    ┌────────┐    ┌────────┐    ┌────────────┐         │
│    │ Policy │    │Gateway │    │ Search │    │   Audit    │         │
│    │ Reload │    │ Reload │    │ Param  │    │  (async)   │         │
│    │  Hook  │    │  Hook  │    │  Hook  │    │            │         │
│    └────────┘    └────────┘    └────────┘    └────────────┘         │
│         │                                                            │
│         ↓ (if Redis enabled)                                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │              Redis Pub/Sub (octofhir:resource_events)          │  │
│  │                   → other server instances                     │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. Event Types (`octofhir-core/src/events/types.rs`)

```rust
/// Unified event enum for all system events
pub enum SystemEvent {
    Resource(ResourceEvent),
    Auth(AuthEvent),
}

/// Resource change event
pub struct ResourceEvent {
    pub event_type: ResourceEventType,  // Created, Updated, Deleted
    pub resource_type: String,
    pub resource_id: String,
    pub version_id: Option<i64>,
    pub resource: Option<Value>,  // None for deletes
    pub timestamp: OffsetDateTime,
}

/// Auth-related events (sessions, tokens, logins)
pub struct AuthEvent {
    pub event_type: AuthEventType,
    pub user_id: Option<String>,
    pub client_id: String,
    pub session_id: Option<String>,
    pub ip_address: Option<IpAddr>,
    pub reason: Option<String>,
    pub timestamp: OffsetDateTime,
}
```

### 2. EventBroadcaster (`octofhir-core/src/events/broadcaster.rs`)

Central event bus using `tokio::broadcast`:

```rust
pub struct EventBroadcaster {
    sender: broadcast::Sender<SystemEvent>,
}

impl EventBroadcaster {
    pub fn new() -> Self;
    pub fn new_shared() -> Arc<Self>;

    // Send events
    pub fn send(&self, event: SystemEvent) -> usize;
    pub fn send_resource(&self, event: ResourceEvent) -> usize;
    pub fn send_auth(&self, event: AuthEvent) -> usize;

    // Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent>;
}
```

**Why `tokio::broadcast`?**
- Multiple subscribers receive same event
- Non-blocking sends (returns immediately)
- Configurable buffer size (default: 1024 events)
- Automatic cleanup of slow subscribers

### 3. Hook Trait (`octofhir-core/src/events/hooks.rs`)

```rust
#[async_trait]
pub trait ResourceHook: Send + Sync {
    /// Hook identifier for logging
    fn name(&self) -> &str;

    /// Resource types to match (empty = all)
    fn resource_types(&self) -> &[&str];

    /// Handle the event (async, isolated)
    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError>;
}
```

### 4. HookRegistry (`octofhir-core/src/events/registry.rs`)

```rust
pub struct HookRegistry {
    resource_hooks: RwLock<Vec<Arc<dyn ResourceHook>>>,
    system_hooks: RwLock<Vec<Arc<dyn SystemHook>>>,
}

impl HookRegistry {
    pub fn new() -> Self;
    pub async fn register_resource(&self, hook: Arc<dyn ResourceHook>);
    pub async fn hook_count(&self) -> usize;
}
```

### 5. HookDispatcher (`octofhir-core/src/events/registry.rs`)

Dispatches events to hooks with isolation:

```rust
pub struct HookDispatcher {
    registry: Arc<HookRegistry>,
}

impl HookDispatcher {
    pub async fn run(self, mut receiver: broadcast::Receiver<SystemEvent>) {
        while let Ok(event) = receiver.recv().await {
            self.dispatch(&event).await;
        }
    }

    async fn dispatch(&self, event: &SystemEvent) {
        // Each hook runs in isolated task with:
        // - 30s timeout
        // - Panic recovery via catch_unwind
        // - Error logging (not propagation)
    }
}
```

### 6. EventedStorage (`octofhir-storage/src/evented.rs`)

Wrapper that emits events after CRUD operations:

```rust
pub struct EventedStorage {
    inner: DynStorage,
    broadcaster: Arc<EventBroadcaster>,
}

impl FhirStorage for EventedStorage {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.create(resource).await?;
        self.broadcaster.send_resource(ResourceEvent::created(...));
        Ok(result)
    }
    // update, delete similarly...
}
```

## Hook Implementations

### PolicyReloadHook (`octofhir-server/src/hooks/policy.rs`)

Triggers policy cache reload on AccessPolicy changes:

```rust
pub struct PolicyReloadHook {
    notifier: Arc<PolicyChangeNotifier>,
}

impl ResourceHook for PolicyReloadHook {
    fn name(&self) -> &str { "policy_reload" }
    fn resource_types(&self) -> &[&str] { &["AccessPolicy"] }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let change = match event.event_type {
            Created => PolicyChange::Created { policy_id: ... },
            Updated => PolicyChange::Updated { policy_id: ... },
            Deleted => PolicyChange::Deleted { policy_id: ... },
        };
        self.notifier.notify(change);  // Debounced
        Ok(())
    }
}
```

### GatewayReloadHook (`octofhir-server/src/hooks/gateway.rs`)

Reloads gateway routes on App/CustomOperation changes:

```rust
pub struct GatewayReloadHook {
    gateway_router: Arc<GatewayRouter>,
    storage: DynStorage,
    operation_registry: Option<Arc<OperationRegistryService>>,
}

impl ResourceHook for GatewayReloadHook {
    fn resource_types(&self) -> &[&str] {
        &["App", "CustomOperation"]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        // Reload gateway routes from storage
        self.gateway_router.reload(&self.storage).await?;

        // Reload operation registry if available
        if let Some(ref registry) = self.operation_registry {
            registry.reload().await?;
        }
        Ok(())
    }
}
```

### SearchParamHook (`octofhir-server/src/hooks/search.rs`)

Updates search parameter registry:

```rust
pub struct SearchParamHook {
    search_config: ReloadableSearchConfig,
}

impl ResourceHook for SearchParamHook {
    fn resource_types(&self) -> &[&str] { &["SearchParameter"] }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        match event.event_type {
            Created | Updated => {
                if let Some(resource) = &event.resource {
                    let param = parse_search_parameter(resource)?;
                    self.search_config.config().registry.upsert(param);
                }
            }
            Deleted => {
                // Registry handles deletion by URL
            }
        }
        // Clear query cache
        self.search_config.clear_cache();
        Ok(())
    }
}
```

### GraphQLSubscriptionHook (`octofhir-server/src/hooks/graphql.rs`)

Forwards events to GraphQL subscription clients:

```rust
pub struct GraphQLSubscriptionHook {
    broadcaster: Arc<ResourceEventBroadcaster>,  // GraphQL broadcaster
}

impl ResourceHook for GraphQLSubscriptionHook {
    fn resource_types(&self) -> &[&str] { &[] }  // All types

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let graphql_event = ResourceChangeEvent::new(
            convert_event_type(event.event_type),
            &event.resource_type,
            &event.resource_id,
            event.resource.clone(),
        );
        self.broadcaster.send(graphql_event);
        Ok(())
    }
}
```

### AsyncAuditHook (`octofhir-server/src/hooks/audit.rs`)

Logs resource changes as AuditEvents (fire-and-forget):

```rust
pub struct AsyncAuditHook {
    audit_service: Arc<AuditService>,
}

impl ResourceHook for AsyncAuditHook {
    fn resource_types(&self) -> &[&str] { &[] }  // All types

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let action = match event.event_type {
            Created => AuditAction::ResourceCreate,
            Updated => AuditAction::ResourceUpdate,
            Deleted => AuditAction::ResourceDelete,
        };

        if !self.audit_service.should_log(&action, Some(&event.resource_type)) {
            return Ok(());
        }

        let audit_builder = AuditEventBuilder::new(action)
            .outcome(AuditOutcome::Success)
            .system()
            .entity(Some(event.resource_type.clone()), ...);

        // Fire-and-forget
        let service = self.audit_service.clone();
        tokio::spawn(async move {
            let _ = service.log(audit_builder).await;
        });

        Ok(())
    }
}
```

## Redis Multi-Instance Sync

### Components (`octofhir-server/src/events/`)

**RedisPublishHook** - Publishes events to Redis:

```rust
pub struct RedisPublishHook {
    pool: Pool,
}

impl ResourceHook for RedisPublishHook {
    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let message = serde_json::to_string(&SerializableEvent::from(event))?;
        conn.publish("octofhir:resource_events", &message).await?;
        Ok(())
    }
}
```

**RedisEventSync** - Subscribes and forwards to local broadcaster:

```rust
pub struct RedisEventSync {
    pool: Pool,
    broadcaster: Arc<EventBroadcaster>,
    redis_url: String,
}

impl RedisEventSync {
    pub async fn run(self: Arc<Self>) {
        loop {
            match self.subscribe_loop().await {
                Ok(()) => break,  // Graceful shutdown
                Err(e) => {
                    error!("Redis sync error, reconnecting in 5s: {}", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn subscribe_loop(&self) -> Result<(), RedisEventError> {
        let mut pubsub = client.get_async_pubsub().await?;
        pubsub.subscribe("octofhir:resource_events").await?;

        while let Some(msg) = stream.next().await {
            let payload: String = msg.get_payload()?;
            let event: SerializableEvent = serde_json::from_str(&payload)?;
            self.broadcaster.send_resource(event.into_resource_event());
        }
        Ok(())
    }
}
```

### Multi-Instance Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│  Instance 1                        Instance 2                       │
│  ┌───────────────┐                ┌───────────────┐                │
│  │ CRUD Operation│                │ CRUD Operation│                │
│  │      ↓        │                │      ↓        │                │
│  │EventedStorage │                │EventedStorage │                │
│  │      ↓        │                │      ↓        │                │
│  │EventBroadcaster─────────────────EventBroadcaster               │
│  │      ↓        │                │      ↑        │                │
│  │  Local Hooks  │                │  Local Hooks  │                │
│  │      ↓        │                │      │        │                │
│  │RedisPublishHook─→ Redis ←─RedisEventSync                       │
│  └───────────────┘   Channel      └───────────────┘                │
└─────────────────────────────────────────────────────────────────────┘
```

## Initialization in server.rs

```rust
// Create event broadcaster
let event_broadcaster = EventBroadcaster::new_shared();

// Wrap storage with event emission
let evented_storage = EventedStorage::new(pg_storage, event_broadcaster.clone());
let storage: DynStorage = Arc::new(evented_storage);

// Create hook registry
let hook_registry = HookRegistry::new();

// Register hooks
hook_registry.register_resource(Arc::new(PolicyReloadHook::new(...))).await;
hook_registry.register_resource(Arc::new(GatewayReloadHook::new(...))).await;
hook_registry.register_resource(Arc::new(SearchParamHook::new(...))).await;

// GraphQL subscription hook (optional)
if let Some(ref broadcaster) = graphql_subscription_broadcaster {
    hook_registry.register_resource(Arc::new(GraphQLSubscriptionHook::new(...))).await;
}

// Async audit hook (if enabled)
if audit_service.is_enabled() {
    hook_registry.register_resource(Arc::new(AsyncAuditHook::new(...))).await;
}

// Redis sync (if enabled)
if cfg.redis.enabled {
    if let Ok(redis_pool) = create_redis_pool(&cfg.redis).await {
        hook_registry.register_resource(Arc::new(RedisPublishHook::new(...))).await;
        RedisEventSyncBuilder::new()
            .with_pool(redis_pool)
            .with_broadcaster(event_broadcaster.clone())
            .with_redis_url(&cfg.redis.url)
            .start()?;
    }
}

// Start dispatcher
let dispatcher = HookDispatcher::new(hook_registry.clone());
tokio::spawn(dispatcher.run(event_broadcaster.subscribe()));
```

## Hook Isolation

Each hook runs with:

1. **Timeout** - 30 seconds max
2. **Panic recovery** - `catch_unwind` wraps handler
3. **Error isolation** - Errors are logged, not propagated
4. **Independent execution** - One hook failure doesn't affect others

```rust
async fn dispatch_to_hook(hook: Arc<dyn ResourceHook>, event: ResourceEvent) {
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        std::panic::AssertUnwindSafe(hook.handle(&event)).catch_unwind()
    ).await;

    match result {
        Ok(Ok(Ok(()))) => { /* success */ }
        Ok(Ok(Err(e))) => warn!(hook = hook.name(), "Hook error: {}", e),
        Ok(Err(panic)) => error!(hook = hook.name(), "Hook panicked!"),
        Err(_) => error!(hook = hook.name(), "Hook timed out"),
    }
}
```

## Conditional Hook Registration

Hooks are only registered if features are enabled:

```rust
// GraphQL subscriptions - only if enabled
if cfg.graphql.subscriptions_enabled {
    hook_registry.register_resource(Arc::new(GraphQLSubscriptionHook::new(...))).await;
}

// Audit - only if enabled
if audit_service.is_enabled() {
    hook_registry.register_resource(Arc::new(AsyncAuditHook::new(...))).await;
}

// Redis sync - only if Redis enabled
if cfg.redis.enabled {
    // Register RedisPublishHook and start RedisEventSync
}

// Future: Licensed modules
if license.has_feature("notifications") {
    hook_registry.register_resource(Arc::new(NotificationsHook::new(...))).await;
}
```

## Performance Characteristics

| Operation | Time | Notes |
|-----------|------|-------|
| Event broadcast | ~1-5 μs | Non-blocking, returns immediately |
| Hook dispatch (per hook) | ~10-100 μs | Plus async execution time |
| Redis publish | ~1-5 ms | Network round-trip |
| Redis subscribe | Continuous | Background task |

## Configuration

```toml
[redis]
enabled = true
url = "redis://localhost:6379"
pool_size = 10
timeout_ms = 5000
```

## Testing

Unit tests for event system:

```bash
# Core event tests
cargo test -p octofhir-core --lib events

# Hook tests
cargo test -p octofhir-server --lib hooks

# Events module tests
cargo test -p octofhir-server --lib events
```

## Benefits vs PostgreSQL LISTEN/NOTIFY

| Aspect | PostgreSQL Listeners | Event System |
|--------|---------------------|--------------|
| Connections | 1 per listener | 0 (app-level) |
| Extensibility | New listener = new code | New hook = one file |
| Audit | Sync, blocks API | Async, fire-and-forget |
| Multi-instance | Separate listeners | Unified Redis channel |
| Testing | Requires PostgreSQL | Mock broadcaster |
| Error handling | Complex reconnect | Isolated per hook |

## Files Changed

### Created

| Path | Description |
|------|-------------|
| `octofhir-core/src/events/mod.rs` | Module exports |
| `octofhir-core/src/events/types.rs` | Event types (ResourceEvent, AuthEvent) |
| `octofhir-core/src/events/broadcaster.rs` | EventBroadcaster |
| `octofhir-core/src/events/hooks.rs` | Hook traits |
| `octofhir-core/src/events/registry.rs` | HookRegistry, HookDispatcher |
| `octofhir-storage/src/evented.rs` | EventedStorage wrapper |
| `octofhir-server/src/hooks/mod.rs` | Hook module |
| `octofhir-server/src/hooks/policy.rs` | PolicyReloadHook |
| `octofhir-server/src/hooks/gateway.rs` | GatewayReloadHook |
| `octofhir-server/src/hooks/search.rs` | SearchParamHook |
| `octofhir-server/src/hooks/graphql.rs` | GraphQLSubscriptionHook |
| `octofhir-server/src/hooks/audit.rs` | AsyncAuditHook |
| `octofhir-server/src/events/mod.rs` | Redis sync module |
| `octofhir-server/src/events/redis.rs` | RedisEventSync |

### Modified

| Path | Changes |
|------|---------|
| `octofhir-core/src/lib.rs` | Added `pub mod events` |
| `octofhir-storage/src/lib.rs` | Added `pub mod evented` |
| `octofhir-server/src/lib.rs` | Added `pub mod events`, `pub mod hooks` |
| `octofhir-server/src/server.rs` | Initialize event system, register hooks |
| `octofhir-auth/src/http/logout.rs` | TokenRevokedCallback for JWT cache |
