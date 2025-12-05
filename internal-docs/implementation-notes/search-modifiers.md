# Search Modifier Implementation Notes

## Overview

This document describes how FHIR search modifiers are implemented in OctoFHIR's search engine.

## File Locations

- **Parser**: `octofhir-search/src/parameters.rs`
- **SQL Builder**: `octofhir-search/src/sql_builder.rs`
- **Type handlers**: `octofhir-search/src/types/*.rs`

## Modifier Processing Flow

```
1. Parse query string
   └─> Extract parameter name, modifier, value

2. Look up SearchParameter definition
   └─> Get parameter type (string, token, date, etc.)

3. Route to type-specific handler
   └─> string.rs, token.rs, date.rs, etc.

4. Generate SQL condition
   └─> Add to SqlBuilder

5. Build final query
   └─> Combine all conditions with AND
```

## String Modifiers

### Implementation (`types/string.rs`)

```rust
pub fn build_string_condition(
    param: &str,
    modifier: Option<&str>,
    value: &str,
    paths: &[String],
) -> (String, Vec<SqlParam>) {
    match modifier {
        None => {
            // Default: starts-with (case-insensitive)
            // SQL: LOWER(path) LIKE LOWER($1) || '%'
        }
        Some("exact") => {
            // Case-sensitive exact match
            // SQL: path = $1
        }
        Some("contains") => {
            // Substring match (case-insensitive)
            // SQL: LOWER(path) LIKE '%' || LOWER($1) || '%'
        }
        Some("missing") => {
            // Element presence
            // SQL: path IS NULL / path IS NOT NULL
        }
        _ => {
            // Unknown modifier - error or ignore
        }
    }
}
```

### JSON Path Extraction

For FHIR elements, we extract from JSONB:

```sql
-- Simple element
resource->>'family'

-- Nested element
resource->'name'->0->>'family'

-- Array element (any match)
EXISTS (SELECT 1 FROM jsonb_array_elements(resource->'name') n
        WHERE n->>'family' ILIKE $1 || '%')
```

## Token Modifiers

### Implementation (`types/token.rs`)

```rust
pub fn build_token_condition(
    param: &str,
    modifier: Option<&str>,
    value: &str,
    paths: &[String],
) -> (String, Vec<SqlParam>) {
    // Parse system|code format
    let (system, code) = parse_token_value(value);

    match modifier {
        None => {
            // Exact match on system and/or code
        }
        Some("text") => {
            // Match on display text
            // SQL: coding->>'display' ILIKE '%' || $1 || '%'
        }
        Some("not") => {
            // Exclude matching codes
            // SQL: NOT (condition)
        }
        Some("of-type") => {
            // Match identifier by type
            // SQL: identifier @> '[{"type": {"coding": [...]}}]'
        }
        Some("in") => {
            // Code in ValueSet - delegate to terminology
        }
        Some("not-in") => {
            // Code not in ValueSet
        }
        Some("below") => {
            // Code or descendants - delegate to terminology
        }
        Some("above") => {
            // Code or ancestors - delegate to terminology
        }
    }
}
```

### Terminology Modifiers

For `:in`, `:not-in`, `:below`, `:above`:

```rust
// In token.rs
if matches!(modifier, Some("in") | Some("not-in") | Some("below") | Some("above")) {
    return build_token_search_with_terminology(
        param, modifier, value, paths, pool, terminology_provider
    ).await;
}

async fn build_token_search_with_terminology(...) -> ... {
    let expansion = match modifier {
        Some("in") | Some("not-in") => {
            terminology_provider.expand_valueset_for_search(pool, value, None).await?
        }
        Some("below") | Some("above") => {
            let (system, code) = parse_token_value(value);
            let direction = if modifier == Some("below") {
                HierarchyDirection::Below
            } else {
                HierarchyDirection::Above
            };
            terminology_provider.expand_hierarchy(system, code, direction).await?
        }
    };

    let condition = match modifier {
        Some("not-in") => format!("NOT ({})", build_expansion_condition(&expansion)),
        _ => build_expansion_condition(&expansion),
    };

    Ok((condition, params))
}
```

## Date Modifiers

### Prefix Parsing (`types/date.rs`)

```rust
fn parse_date_prefix(value: &str) -> (DatePrefix, &str) {
    if value.starts_with("eq") { return (DatePrefix::Eq, &value[2..]); }
    if value.starts_with("ne") { return (DatePrefix::Ne, &value[2..]); }
    if value.starts_with("lt") { return (DatePrefix::Lt, &value[2..]); }
    if value.starts_with("le") { return (DatePrefix::Le, &value[2..]); }
    if value.starts_with("gt") { return (DatePrefix::Gt, &value[2..]); }
    if value.starts_with("ge") { return (DatePrefix::Ge, &value[2..]); }
    if value.starts_with("sa") { return (DatePrefix::Sa, &value[2..]); }
    if value.starts_with("eb") { return (DatePrefix::Eb, &value[2..]); }
    if value.starts_with("ap") { return (DatePrefix::Ap, &value[2..]); }
    (DatePrefix::Eq, value) // Default is equality
}
```

### Precision-Aware Comparison

```rust
fn build_date_condition(prefix: DatePrefix, date: &str, path: &str) -> String {
    let (start, end) = expand_date_to_range(date);
    // date "1980" expands to "1980-01-01" to "1980-12-31"
    // date "1980-01" expands to "1980-01-01" to "1980-01-31"

    match prefix {
        DatePrefix::Eq => format!("{} >= $start AND {} <= $end", path, path),
        DatePrefix::Ne => format!("NOT ({} >= $start AND {} <= $end)", path, path),
        DatePrefix::Lt => format!("{} < $start", path),
        DatePrefix::Le => format!("{} <= $end", path),
        DatePrefix::Gt => format!("{} > $end", path),
        DatePrefix::Ge => format!("{} >= $start", path),
        DatePrefix::Sa => format!("{} > $end", path),
        DatePrefix::Eb => format!("{} < $start", path),
        DatePrefix::Ap => {
            // Approximately - expand range by 10%
            let range = end - start;
            let margin = range / 10;
            format!("{} >= $adjusted_start AND {} <= $adjusted_end", path, path)
        }
    }
}
```

## Missing Modifier

### Universal Implementation

The `:missing` modifier works on any parameter type:

```rust
fn build_missing_condition(paths: &[String], is_missing: bool) -> String {
    let conditions: Vec<String> = paths.iter().map(|path| {
        if is_missing {
            format!("({} IS NULL OR {} = 'null'::jsonb)", path, path)
        } else {
            format!("({} IS NOT NULL AND {} != 'null'::jsonb)", path, path)
        }
    }).collect();

    if is_missing {
        // All paths must be missing
        conditions.join(" AND ")
    } else {
        // Any path must exist
        conditions.join(" OR ")
    }
}
```

## Include/RevInclude

### Implementation (`include.rs`)

```rust
pub async fn process_includes(
    base_results: &[StoredResource],
    includes: &[IncludeParam],
    storage: &dyn Storage,
) -> Result<Vec<StoredResource>, SearchError> {
    let mut included = Vec::new();

    for include in includes {
        let refs = extract_references(base_results, &include.search_param);
        let resources = storage.read_many(&refs).await?;
        included.extend(resources);
    }

    Ok(included)
}

pub async fn process_revincludes(
    base_results: &[StoredResource],
    revincludes: &[IncludeParam],
    storage: &dyn Storage,
) -> Result<Vec<StoredResource>, SearchError> {
    let mut included = Vec::new();

    for revinclude in revincludes {
        let base_refs: Vec<String> = base_results.iter()
            .map(|r| format!("{}/{}", r.resource_type, r.id))
            .collect();

        let query = format!(
            "{}?{}={}",
            revinclude.resource_type,
            revinclude.search_param,
            base_refs.join(",")
        );

        let results = search(&query, storage).await?;
        included.extend(results);
    }

    Ok(included)
}
```

## Elements/Summary Filtering

### Implementation (`handlers.rs`)

```rust
fn filter_elements(resource: &mut Value, elements: &[String]) {
    // Always keep: resourceType, id, meta
    let keep: HashSet<_> = ["resourceType", "id", "meta"]
        .iter()
        .chain(elements.iter().map(|s| s.as_str()))
        .collect();

    if let Some(obj) = resource.as_object_mut() {
        obj.retain(|key, _| keep.contains(key.as_str()));
    }

    // Add SUBSETTED tag
    add_subsetted_tag(resource);
}

fn apply_summary(resource: &mut Value, summary: &str) {
    match summary {
        "true" => {
            // Keep only summary elements (from StructureDefinition)
            filter_to_summary_elements(resource);
        }
        "text" => {
            // Keep text, id, meta
            filter_to_text_elements(resource);
        }
        "data" => {
            // Remove text element
            remove_text_element(resource);
        }
        "count" => {
            // Return nothing (handled at bundle level)
        }
        _ => {}
    }
}
```

## Error Handling

### Unknown Modifier

```rust
if !is_valid_modifier(modifier, param_type) {
    return Err(SearchError::InvalidModifier {
        modifier: modifier.to_string(),
        parameter: param.to_string(),
        allowed: get_allowed_modifiers(param_type),
    });
}
```

### Terminology Errors

```rust
match terminology_provider.expand_valueset(url).await {
    Ok(expansion) => { /* proceed */ }
    Err(TerminologyError::NotFound) => {
        return Err(SearchError::ValueSetNotFound(url.to_string()));
    }
    Err(TerminologyError::ServiceUnavailable) => {
        return Err(SearchError::TerminologyUnavailable);
    }
    Err(e) => {
        return Err(SearchError::TerminologyError(e.to_string()));
    }
}
```

## Testing

Each modifier has unit tests:

```rust
#[test]
fn test_string_exact_modifier() {
    let (sql, params) = build_string_condition(
        "family", Some("exact"), "Smith", &["resource->>'family'"]
    );
    assert_eq!(sql, "resource->>'family' = $1");
    assert_eq!(params, vec![SqlParam::String("Smith".to_string())]);
}
```

Integration tests verify end-to-end behavior:

```rust
#[tokio::test]
async fn test_string_exact_modifier_integration() {
    // Setup server with test data
    // Execute search
    // Verify results
}
```
