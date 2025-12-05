# PostgreSQL Transaction Architecture

## Overview

OctoFHIR provides native PostgreSQL transaction support for FHIR Bundle transactions, ensuring ACID guarantees for multi-resource operations.

## Components

### PostgresTransaction (`octofhir-db-postgres/src/transaction.rs`)

```rust
pub struct PostgresTransaction {
    tx: Mutex<Option<Box<PgTransaction<'static>>>>,
    schema_manager: SchemaManager,
}
```

Key design decisions:
- `Mutex` for interior mutability (sqlx requires `&mut` for all operations)
- `Option` to allow ownership transfer during commit/rollback
- `Box` to avoid lifetime parameters
- `'static` lifetime via ownership transfer from pool

### Transaction Trait (`octofhir-storage/src/traits.rs`)

```rust
#[async_trait]
pub trait Transaction: Send + Sync {
    async fn commit(mut self: Box<Self>) -> Result<(), StorageError>;
    async fn rollback(mut self: Box<Self>) -> Result<(), StorageError>;

    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError>;
    async fn update(&mut self, resource: &Value) -> Result<StoredResource, StorageError>;
    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError>;
    async fn read(&self, resource_type: &str, id: &str) -> Result<Option<StoredResource>, StorageError>;
}
```

## Transaction Lifecycle

```
1. Begin Transaction
   └─> Pool.begin() returns PgTransaction
   └─> Wrap in PostgresTransaction

2. Execute Operations
   └─> Each operation uses transaction connection
   └─> Changes visible within transaction
   └─> Isolated from other transactions

3a. Commit
    └─> tx.commit().await
    └─> Changes persisted
    └─> Transaction closed

3b. Rollback (explicit or on drop)
    └─> tx.rollback().await
    └─> All changes discarded
    └─> Transaction closed
```

## Bundle Processing Flow

### Transaction Bundle (`handlers.rs`)

```rust
async fn process_transaction(state: &AppState, bundle: &Value) -> Result<...> {
    // 1. Begin transaction
    let tx = state.storage.begin_transaction().await?;

    // 2. Process each entry within transaction
    let mut response_entries = Vec::new();
    let mut reference_map = HashMap::new();

    for entry in entries {
        match process_entry_with_tx(&mut *tx, entry, &mut reference_map).await {
            Ok(response) => response_entries.push(response),
            Err(e) => {
                // 3a. Rollback on any failure
                tx.rollback().await?;
                return Err(e);
            }
        }
    }

    // 3b. Commit on success
    tx.commit().await?;

    Ok(build_response(response_entries))
}
```

### Batch Bundle (Independent Operations)

```rust
async fn process_batch(state: &AppState, bundle: &Value) -> Result<...> {
    let mut response_entries = Vec::new();

    for entry in entries {
        // Each entry is independent - no transaction
        let result = process_entry(state, entry).await;
        response_entries.push(match result {
            Ok(r) => r,
            Err(e) => build_error_response(e),
        });
    }

    Ok(build_response(response_entries))
}
```

## Reference Resolution

### Internal References (urn:uuid)

```rust
fn resolve_references(resource: &mut Value, reference_map: &HashMap<String, String>) {
    // Walk JSON tree
    // Find "reference" fields with "urn:uuid:" prefix
    // Replace with actual resource reference from map
}
```

### Reference Map Building

```rust
// After creating a resource:
if let Some(full_url) = entry["fullUrl"].as_str() {
    if full_url.starts_with("urn:uuid:") {
        let actual_ref = format!("{}/{}", resource_type, created_id);
        reference_map.insert(full_url.to_string(), actual_ref);
    }
}
```

## Isolation Levels

PostgreSQL default: READ COMMITTED

Behavior:
- Each statement sees committed data at statement start
- Uncommitted changes from other transactions not visible
- Our transaction's changes visible to subsequent operations

For stricter isolation, use:
```sql
SET TRANSACTION ISOLATION LEVEL SERIALIZABLE;
```

## Error Handling

### Automatic Rollback on Drop

```rust
impl Drop for PostgresTransaction {
    fn drop(&mut self) {
        if self.tx.blocking_lock().is_some() {
            tracing::warn!("Transaction dropped without commit/rollback");
            // sqlx Transaction Drop automatically rolls back
        }
    }
}
```

### Serialization Failures

When concurrent transactions conflict:
1. PostgreSQL raises serialization error
2. Converted to `StorageError::TransactionError`
3. Handler returns 409 Conflict
4. Client should retry with fresh data

## Concurrency Patterns

### Optimistic Locking

Using version IDs (versionId from meta):
```rust
if let Some(if_match) = headers.get("If-Match") {
    let current_version = get_current_version(resource_type, id).await?;
    if current_version != expected_version {
        return Err(ApiError::conflict("Version mismatch"));
    }
}
```

### Conditional Create

Using If-None-Exist:
```rust
if let Some(search) = request["ifNoneExist"].as_str() {
    let existing = search_for_match(search).await?;
    if !existing.is_empty() {
        return Ok(existing[0].clone()); // Return existing instead of create
    }
}
```

## Performance Considerations

### Transaction Duration

Keep transactions short:
- Pre-validate all resources before beginning
- Resolve references after building reference map
- Avoid external calls within transaction

### Connection Pool Impact

Each transaction holds a connection:
- Long transactions reduce available connections
- Monitor pool usage
- Set reasonable timeouts

### Large Transactions

For many entries (>100):
- Consider chunking if atomicity not required
- Use batch for independent operations
- Monitor transaction duration

## Testing

Integration tests in `integration_transactions.rs`:
- `test_transaction_commit_all_creates`
- `test_transaction_rollback_on_invalid_reference`
- `test_concurrent_transactions_isolation`
- `test_batch_partial_failure`
- `test_transaction_internal_reference_resolution`
- `test_large_transaction_performance`
