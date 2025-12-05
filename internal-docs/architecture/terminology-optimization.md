# Terminology Service Optimization

## Overview

OctoFHIR optimizes terminology-based searches (`:in`, `:not-in`, `:below`, `:above`) using automatic strategy selection based on expansion size.

## Components

### Terminology Provider (`octofhir-search/src/terminology.rs`)

```rust
pub struct HybridTerminologyProvider {
    canonical_manager: Arc<CanonicalManager>,
    config: TerminologyConfig,
    cache: Arc<DashMap<String, CachedExpansion>>,
}
```

### Expansion Result

```rust
pub enum ExpansionResult {
    /// Small expansion - use IN clause
    InClause(Vec<CodeSystemCode>),

    /// Large expansion - use temp table
    TempTable {
        session_id: String,
        code_count: usize,
    },
}
```

## Optimization Strategy

### Threshold-Based Selection

```rust
const TEMP_TABLE_THRESHOLD: usize = 500;

pub async fn expand_valueset_for_search(
    &self,
    pool: &PgPool,
    valueset_url: &str,
    filter: Option<&str>,
) -> Result<ExpansionResult, TerminologyError> {
    let codes = self.expand_valueset(valueset_url, filter).await?;

    if codes.len() < TEMP_TABLE_THRESHOLD {
        Ok(ExpansionResult::InClause(codes))
    } else {
        let session_id = self.bulk_insert_to_temp_table(pool, &codes).await?;
        Ok(ExpansionResult::TempTable { session_id, code_count: codes.len() })
    }
}
```

### Why 500?

- PostgreSQL IN clause performance degrades significantly beyond ~500 values
- Temp table JOIN with index scales linearly
- 500 provides good balance for most ValueSets

## Temp Table Implementation

### Schema (`migrations/20241201000006_temp_valueset_tables.sql`)

```sql
CREATE UNLOGGED TABLE IF NOT EXISTS temp_valueset_codes (
    session_id TEXT NOT NULL,
    code TEXT NOT NULL,
    system TEXT,
    display TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_temp_valueset_session_code
    ON temp_valueset_codes(session_id, code, system);

CREATE INDEX idx_temp_valueset_cleanup
    ON temp_valueset_codes(created_at);
```

Key design decisions:
- `UNLOGGED` for performance (no WAL overhead)
- Session-based isolation
- Cleanup index for TTL enforcement

### Bulk Insert

```rust
async fn bulk_insert_to_temp_table(
    &self,
    pool: &PgPool,
    codes: &[CodeSystemCode],
) -> Result<String, TerminologyError> {
    let session_id = Uuid::new_v4().to_string();

    // Use COPY for bulk insert (much faster than INSERT)
    let mut copy = pool
        .copy_in_raw("COPY temp_valueset_codes (session_id, code, system, display, created_at) FROM STDIN")
        .await?;

    for code in codes {
        let row = format!(
            "{}\t{}\t{}\t{}\t{}\n",
            session_id,
            code.code,
            code.system.as_deref().unwrap_or(""),
            code.display.as_deref().unwrap_or(""),
            Utc::now().to_rfc3339()
        );
        copy.send(row.as_bytes()).await?;
    }

    copy.finish().await?;
    Ok(session_id)
}
```

### Cleanup

```sql
CREATE OR REPLACE FUNCTION cleanup_temp_valueset_codes()
RETURNS void AS $$
BEGIN
    DELETE FROM temp_valueset_codes
    WHERE created_at < NOW() - INTERVAL '1 hour';
END;
$$ LANGUAGE plpgsql;
```

Cleanup is triggered:
- Periodically via background job
- On server startup
- When temp table grows too large

## SQL Generation

### SqlBuilder Integration (`sql_builder.rs`)

```rust
impl SqlBuilder {
    pub fn add_valueset_condition(
        &mut self,
        json_path: &str,
        expansion: &ExpansionResult,
    ) {
        match expansion {
            ExpansionResult::InClause(codes) => {
                // Generate: code_value IN ($1, $2, ..., $n)
                let placeholders = self.add_string_params(
                    codes.iter().map(|c| c.code.as_str())
                );
                self.conditions.push(format!(
                    "{} IN ({})", json_path, placeholders
                ));
            }
            ExpansionResult::TempTable { session_id, .. } => {
                // Generate: EXISTS (SELECT 1 FROM temp_valueset_codes t
                //           WHERE t.session_id = $n AND t.code = ...)
                let param = self.add_string_param(session_id);
                self.conditions.push(format!(
                    "EXISTS (SELECT 1 FROM temp_valueset_codes t \
                     WHERE t.session_id = {} AND t.code = {})",
                    param, json_path
                ));
            }
        }
    }
}
```

## Hierarchy Expansion

### SNOMED CT with ECL

```rust
pub enum HierarchyDirection {
    Below,  // Descendants (<<)
    Above,  // Ancestors (>>)
}

async fn expand_snomed_hierarchy(
    &self,
    code: &str,
    direction: HierarchyDirection,
) -> Result<Vec<String>, TerminologyError> {
    let ecl = match direction {
        HierarchyDirection::Below => format!("<< {}", code),
        HierarchyDirection::Above => format!(">> {}", code),
    };

    let encoded = urlencoding::encode(&ecl);
    let url = format!(
        "{}/ValueSet/$expand?url=http://snomed.info/sct?fhir_vs=ecl/{}",
        self.config.server_url,
        encoded
    );

    self.fetch_expansion(&url).await
}
```

### Generic System Fallback

```rust
async fn expand_remote_hierarchy(
    &self,
    system: &str,
    code: &str,
    direction: HierarchyDirection,
) -> Result<Vec<String>, TerminologyError> {
    // Try to use terminology server's $subsumes operation
    // Fall back to returning just the code if unsupported
    match self.try_subsumes(system, code, direction).await {
        Ok(codes) => Ok(codes),
        Err(_) => {
            tracing::warn!("Hierarchy expansion not supported for {}", system);
            Ok(vec![code.to_string()])
        }
    }
}
```

## Caching

### Cache Structure

```rust
struct CachedExpansion {
    codes: Vec<CodeSystemCode>,
    cached_at: Instant,
}
```

### Cache Key

```
"{valueset_url}:{filter}"
```

### TTL Enforcement

```rust
fn is_valid(&self, ttl_secs: u64) -> bool {
    self.cached_at.elapsed().as_secs() < ttl_secs
}
```

## Performance Characteristics

### Small ValueSet (< 500 codes)

```
Expand: 50-500ms (remote call)
Cache hit: < 1ms
SQL: IN ($1, $2, ...) - single index scan
```

### Large ValueSet (>= 500 codes)

```
Expand: 100-2000ms (remote call)
Bulk insert: 10-50ms (COPY)
SQL: EXISTS with JOIN - index scan on temp table
Cache hit: < 5ms (returns session_id)
```

### Hierarchy Expansion

```
SNOMED CT (ECL): 200-1000ms
Cached: < 5ms
Large hierarchy: Uses temp table strategy
```

## Monitoring

### Metrics

```
octofhir_terminology_expansion_duration_seconds{valueset}
octofhir_terminology_cache_hits_total
octofhir_terminology_cache_misses_total
octofhir_terminology_temp_table_inserts_total
octofhir_terminology_temp_table_size_bytes
```

### Logging

```rust
tracing::info!(
    valueset = %url,
    code_count = codes.len(),
    strategy = if codes.len() < 500 { "in_clause" } else { "temp_table" },
    duration_ms = elapsed.as_millis(),
    "ValueSet expanded"
);
```

## Testing

Unit tests:
- `test_small_expansion_uses_in_clause`
- `test_large_expansion_uses_temp_table`
- `test_cache_hit`
- `test_hierarchy_below`
- `test_hierarchy_above`

Integration tests (terminology_integration.rs):
- `test_valueset_in_modifier_small`
- `test_valueset_not_in_modifier`
- `test_snomed_ct_below_modifier`
- `test_large_valueset_uses_temp_table`
- `test_terminology_caching`
