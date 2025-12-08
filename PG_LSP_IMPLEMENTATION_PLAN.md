# PostgreSQL LSP for OctoFHIR DB Console

## Goal

Build a custom PostgreSQL LSP with FHIR-aware JSONB path autocomplete for the DB Console.

## Decisions

- **Architecture**: Embedded in octofhir-server at `/api/pg-lsp` (WebSocket)
- **Parser**: `pg_query = "6.1"` crate (same as Supabase LSP, wraps libpg_query)
- **JSONB Detection**: Auto-detect resource type from table name in canonical manager
- **Scope**: All tables, with FHIR-aware completions for resource tables
- **Authentication**: Required - token sent via WebSocket query param

## Features (v1)

- **Autocomplete**: SQL keywords, tables, columns, JSONB functions
- **JSONB Path Autocomplete**: FHIR element paths from StructureDefinition
- **Hover**: Column types, FHIR element definitions
- **No linting** (v1)

---

## Crate Structure

```text
crates/octofhir-pg-lsp/
├── Cargo.toml
└── src/
    ├── lib.rs                 # Public API
    ├── server.rs              # tower-lsp LanguageServer impl
    ├── transport.rs           # WebSocket adapter
    ├── parser/
    │   ├── mod.rs             # pg_query wrapper
    │   └── context.rs         # Cursor context detection
    ├── schema_cache/
    │   ├── mod.rs             # Schema cache manager
    │   └── introspection.rs   # information_schema queries
    ├── fhir/
    │   ├── mod.rs             # FHIR path resolver
    │   └── element_tree.rs    # StructureDefinition element tree
    ├── providers/
    │   ├── mod.rs             # Provider trait & registry
    │   ├── keywords.rs        # SQL keywords
    │   ├── tables.rs          # Table completions
    │   ├── columns.rs         # Column completions
    │   ├── functions.rs       # PostgreSQL functions
    │   └── jsonb.rs           # JSONB operators & FHIR paths
    └── hover/
        └── mod.rs             # Hover information
```

## Dependencies

```toml
[dependencies]
pg_query = "6.1"              # libpg_query Rust bindings (same as Supabase)
tower-lsp = "0.20"            # LSP server framework
lsp-types = "0.95"            # LSP type definitions
dashmap = "6"                 # Concurrent cache
sqlx = { version = "0.8", features = ["postgres"] }
tokio = { version = "1", features = ["sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
async-trait = "0.1"

# Workspace
octofhir-canonical-manager = { path = "../octofhir-canonical-manager" }
```

---

## Implementation Checklist

### Phase 1: Core Infrastructure

- [x] Create `crates/octofhir-pg-lsp/Cargo.toml` - Using existing lsp module in octofhir-server
- [x] Create `crates/octofhir-pg-lsp/src/lib.rs` with public exports - `lsp/mod.rs`
- [x] Create `crates/octofhir-pg-lsp/src/server.rs` with basic LanguageServer trait - `lsp/server.rs`
- [x] Add `pg_query` to workspace `Cargo.toml`
- [x] Add route `/api/pg-lsp` in `crates/octofhir-server/src/server.rs`

### Phase 2: SQL Parser Integration

- [ ] Create `crates/octofhir-pg-lsp/src/parser/mod.rs` - pg_query wrapper
- [ ] Create `crates/octofhir-pg-lsp/src/parser/context.rs` - cursor context detection
- [ ] Implement `CursorContext` enum (SelectColumns, FromClause, WhereExpression, JsonbPath, etc.)
- [ ] Implement `JsonbOperator` enum (`->`, `->>`, `#>`, `#>>`, `@>`, `?`, `?|`, `?&`)
- [ ] Handle incomplete SQL with error recovery
- [ ] Use `pg_query::scan()` for token positions

### Phase 3: Schema Cache

- [ ] Create `crates/octofhir-pg-lsp/src/schema_cache/mod.rs` - cache manager
- [ ] Create `crates/octofhir-pg-lsp/src/schema_cache/introspection.rs` - DB queries
- [ ] Implement `TableInfo`, `ColumnInfo`, `FunctionInfo` types
- [ ] Query `information_schema.tables` for table list
- [ ] Query `information_schema.columns` for column metadata
- [ ] Query `information_schema.routines` for JSONB functions
- [ ] Implement FHIR table detection via canonical manager
- [ ] Add cache refresh mechanism

### Phase 4: FHIR Path Resolution

- [ ] Create `crates/octofhir-pg-lsp/src/fhir/mod.rs` - resolver entry point
- [ ] Create `crates/octofhir-pg-lsp/src/fhir/element_tree.rs` - element tree builder
- [ ] Implement `ElementTree` from StructureDefinition `snapshot.element[]`
- [ ] Implement `ElementInfo` struct (name, path, type_code, cardinality, description)
- [ ] Implement `children_of()` for path traversal
- [ ] Cache element trees per resource type

### Phase 5: Completion Providers

- [ ] Create `crates/octofhir-pg-lsp/src/providers/mod.rs` - provider trait & registry
- [ ] Create `crates/octofhir-pg-lsp/src/providers/keywords.rs` - SQL keywords
- [ ] Create `crates/octofhir-pg-lsp/src/providers/tables.rs` - table completions
- [ ] Create `crates/octofhir-pg-lsp/src/providers/columns.rs` - column completions
- [ ] Create `crates/octofhir-pg-lsp/src/providers/functions.rs` - PostgreSQL functions
- [ ] Create `crates/octofhir-pg-lsp/src/providers/jsonb.rs` - JSONB operators & FHIR paths
- [ ] Implement `CompletionProvider` trait
- [ ] Implement `CompletionRegistry` for provider aggregation
- [ ] Add all JSONB functions (jsonb_extract_path, jsonb_array_elements, etc.)
- [ ] Add all JSONB operators (`->`, `->>`, `#>`, `#>>`, `@>`, `<@`, `?`, `?|`, `?&`, `||`, `-`, `#-`)

### Phase 6: Hover Service

- [ ] Create `crates/octofhir-pg-lsp/src/hover/mod.rs`
- [ ] Implement column type hover
- [ ] Implement FHIR element definition hover from StructureDefinition
- [ ] Implement function signature hover

### Phase 7: WebSocket Transport with Authentication

- [x] Create `crates/octofhir-pg-lsp/src/transport.rs` - `lsp/handler.rs`
- [x] Implement authenticated WebSocket handler
- [x] Validate token from query param (`?token=xxx`)
- [ ] Check `db_console:access` permission (TODO: policy check)
- [x] Bridge WebSocket messages to tower-lsp
- [x] Register route `/api/pg-lsp` in server.rs
- [x] Handle connection lifecycle and cleanup

### Phase 8: Monaco Integration

- [ ] Create `ui/src/shared/monaco/lsp-client.ts`
- [ ] Implement WebSocket connection with auth token
- [ ] Configure `monaco-languageclient`
- [ ] Modify `ui/src/shared/monaco/SqlEditor.tsx` to use LSP client
- [ ] Handle LSP client lifecycle (start/stop)
- [ ] Test autocomplete trigger characters (`.`, `'`, ` `, `>`, `-`, `#`)

---

## Files to Create

| File | Purpose |
|------|---------|
| `crates/octofhir-pg-lsp/Cargo.toml` | Crate manifest |
| `crates/octofhir-pg-lsp/src/lib.rs` | Public exports |
| `crates/octofhir-pg-lsp/src/server.rs` | LanguageServer impl |
| `crates/octofhir-pg-lsp/src/transport.rs` | WebSocket adapter |
| `crates/octofhir-pg-lsp/src/parser/mod.rs` | pg_query wrapper |
| `crates/octofhir-pg-lsp/src/parser/context.rs` | Cursor context |
| `crates/octofhir-pg-lsp/src/schema_cache/mod.rs` | Cache manager |
| `crates/octofhir-pg-lsp/src/schema_cache/introspection.rs` | DB queries |
| `crates/octofhir-pg-lsp/src/fhir/mod.rs` | FHIR resolver |
| `crates/octofhir-pg-lsp/src/fhir/element_tree.rs` | Element tree |
| `crates/octofhir-pg-lsp/src/providers/mod.rs` | Provider trait |
| `crates/octofhir-pg-lsp/src/providers/keywords.rs` | Keywords |
| `crates/octofhir-pg-lsp/src/providers/tables.rs` | Tables |
| `crates/octofhir-pg-lsp/src/providers/columns.rs` | Columns |
| `crates/octofhir-pg-lsp/src/providers/functions.rs` | Functions |
| `crates/octofhir-pg-lsp/src/providers/jsonb.rs` | JSONB + FHIR paths |
| `crates/octofhir-pg-lsp/src/hover/mod.rs` | Hover info |
| `ui/src/shared/monaco/lsp-client.ts` | Monaco LSP client |

## Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` | Add `octofhir-pg-lsp` to workspace members |
| `crates/octofhir-server/Cargo.toml` | Add `octofhir-pg-lsp` dependency |
| `crates/octofhir-server/src/server.rs` | Add `/api/pg-lsp` route |
| `ui/src/shared/monaco/SqlEditor.tsx` | Integrate LSP client |

---

## Key Code Snippets

### CursorContext Types

```rust
pub enum CursorContext {
    SelectColumns { table_alias: Option<String>, partial: String },
    FromClause { partial: String },
    WhereExpression { partial: String },
    JsonbPath { table: String, column: String, path: Vec<String>, operator: JsonbOperator },
    FunctionArgs { function: String, arg_index: usize },
    Unknown { partial: String },
}

pub enum JsonbOperator {
    Arrow,        // ->
    DoubleArrow,  // ->>
    HashArrow,    // #>
    HashDouble,   // #>>
    Contains,     // @>
    Exists,       // ?
    ExistsAny,    // ?|
    ExistsAll,    // ?&
}
```

### FHIR Table Detection

```rust
fn is_fhir_table(table_name: &str, canonical_manager: &CanonicalManager) -> Option<String> {
    let resource_type = to_pascal_case(table_name);
    if canonical_manager.has_resource_type(&resource_type) {
        Some(resource_type)
    } else {
        None
    }
}
```

### Authenticated WebSocket Handler

```rust
pub async fn pg_lsp_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<LspQueryParams>,
) -> Result<Response, ApiError> {
    let token = params.token.ok_or(ApiError::Unauthorized)?;
    let session = state.auth.validate_token(&token).await?;

    if !session.has_permission("db_console:access") {
        return Err(ApiError::Forbidden);
    }

    Ok(ws.on_upgrade(move |socket| handle_lsp(socket, state, session)))
}
```

### UI Token Handling

```typescript
const token = getAuthToken();
const wsUrl = `${wsProtocol}//${host}/api/pg-lsp?token=${encodeURIComponent(token)}`;
const webSocket = new WebSocket(wsUrl);
```

---

## Reference: Reusing from Supabase LSP

- `pg_query = "6.1"` - Same dependency for libpg_query bindings
- Modular provider pattern - Separate providers for different completion types
- Schema cache design - Query information_schema, cache with DashMap
- tower-lsp - Standard LSP server framework

## Not Reusing

- Tree-sitter - Using pg_query directly is sufficient
- Full 35-crate structure - Simpler single-crate design
- Linting/diagnostics - Not needed for v1
