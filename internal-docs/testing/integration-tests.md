# Integration Testing Guide

## Overview

Integration tests verify end-to-end functionality with a real PostgreSQL database using testcontainers.

## Test Files

```
crates/octofhir-server/tests/
├── integration_search.rs       # Search parameter tests
├── integration_transactions.rs # Transaction tests
├── terminology_integration.rs  # Terminology service tests
├── config_reload.rs           # Configuration hot-reload tests
└── crud_and_search.rs         # Basic CRUD operations
```

## Testcontainers Setup

### Dependencies

```toml
[dev-dependencies]
testcontainers = "0.15"
testcontainers-modules = { version = "0.3", features = ["postgres"] }
tokio = { version = "1", features = ["full", "test-util"] }
reqwest = { version = "0.11", features = ["json"] }
```

### Basic Test Structure

```rust
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

async fn start_postgres() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container");

    let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );

    (container, url)
}

fn create_config(postgres_url: &str) -> AppConfig {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 5,
        connect_timeout_ms: 10000,
        ..Default::default()
    });
    config
}

async fn start_server(config: &AppConfig) -> (String, oneshot::Sender<()>, JoinHandle<()>) {
    let app = build_app(config).await.expect("build app");

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async { let _ = rx.await; })
            .await
            .ok();
    });

    (format!("http://{addr}"), tx, server)
}
```

## Test Patterns

### Search Tests

```rust
#[tokio::test]
async fn test_string_exact_modifier() {
    // Setup
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "Smith"}]
    });
    client.post(format!("{base}/Patient"))
        .json(&patient)
        .send()
        .await
        .expect("create");

    // Execute search
    let resp = client
        .get(format!("{base}/Patient?family:exact=Smith"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search");

    // Verify
    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse");
    assert_eq!(bundle["total"], 1);

    // Cleanup
    let _ = shutdown_tx.send(());
}
```

### Transaction Tests

```rust
#[tokio::test]
async fn test_transaction_rollback() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create transaction with invalid operation
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {"resourceType": "Patient", "name": [{"family": "Test"}]},
                "request": {"method": "POST", "url": "Patient"}
            },
            {
                "request": {"method": "PUT", "url": "Patient/nonexistent"}
            }
        ]
    });

    let resp = client.post(format!("{base}/"))
        .json(&bundle)
        .send()
        .await
        .expect("transaction");

    // Transaction should fail
    assert!(!resp.status().is_success());

    // Patient should NOT exist (rolled back)
    let search = client.get(format!("{base}/Patient?family=Test"))
        .send()
        .await
        .expect("search");
    let bundle: Value = search.json().await.expect("parse");
    assert_eq!(bundle["total"], 0);

    let _ = shutdown_tx.send(());
}
```

### Terminology Tests

```rust
#[tokio::test]
#[ignore] // Requires internet for tx.fhir.org
async fn test_valueset_in_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config(&postgres_url);

    // Enable terminology
    config.terminology = TerminologyConfig {
        enabled: true,
        server_url: "https://tx.fhir.org/r4".to_string(),
        cache_ttl_secs: 300,
    };

    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create observation with known code
    let obs = json!({
        "resourceType": "Observation",
        "status": "final",
        "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]}
    });
    client.post(format!("{base}/Observation")).json(&obs).send().await.ok();

    // Search with :in modifier
    let resp = client
        .get(format!("{base}/Observation?code:in=http://hl7.org/fhir/ValueSet/observation-codes"))
        .send()
        .await
        .expect("search");

    assert!(resp.status().is_success());

    let _ = shutdown_tx.send(());
}
```

## Running Tests

### All Integration Tests

```bash
# Requires Docker running
cargo test -p octofhir-server --test '*'
```

### Specific Test File

```bash
cargo test -p octofhir-server --test integration_search
cargo test -p octofhir-server --test integration_transactions
```

### Ignored Tests (Require External Services)

```bash
# Run terminology tests (need internet)
cargo test -p octofhir-server --test terminology_integration -- --ignored
```

### With Logging

```bash
RUST_LOG=debug cargo test -p octofhir-server --test integration_search -- --nocapture
```

## Test Helpers

### Creating Test Data

```rust
async fn create_test_patients(client: &reqwest::Client, base: &str) -> Vec<String> {
    let mut ids = Vec::new();

    let patients = vec![
        json!({"resourceType": "Patient", "name": [{"family": "Smith"}]}),
        json!({"resourceType": "Patient", "name": [{"family": "Johnson"}]}),
    ];

    for patient in patients {
        let resp = client.post(format!("{base}/Patient"))
            .json(&patient)
            .send()
            .await
            .expect("create");

        if resp.status().is_success() {
            let created: Value = resp.json().await.expect("parse");
            ids.push(created["id"].as_str().unwrap().to_string());
        }
    }

    ids
}
```

### Checking Results

```rust
fn get_bundle_total(bundle: &Value) -> u64 {
    bundle["total"].as_u64().unwrap_or(0)
}

fn get_bundle_entries(bundle: &Value) -> &Vec<Value> {
    static EMPTY: Vec<Value> = Vec::new();
    bundle["entry"].as_array().unwrap_or(&EMPTY)
}

async fn resource_exists(client: &reqwest::Client, base: &str, rt: &str, id: &str) -> bool {
    let resp = client.get(format!("{base}/{rt}/{id}")).send().await.expect("read");
    resp.status().is_success()
}
```

## Common Issues

### Port Conflicts

If tests fail with port binding errors:
- Ensure no other tests running
- Check for orphaned Docker containers

### Slow Tests

Each test starts a new PostgreSQL container:
- Use `#[ignore]` for slow/optional tests
- Consider test fixtures for repeated data

### Container Cleanup

Testcontainers automatically cleans up, but if containers persist:

```bash
docker ps -a | grep postgres | awk '{print $1}' | xargs docker rm -f
```

### Flaky Tests

For timing-sensitive tests:
- Use explicit waits with timeouts
- Retry on transient failures
- Check for race conditions

## Coverage

Run with coverage:

```bash
cargo tarpaulin -p octofhir-server --test '*' --out Html
```

Target: >80% coverage for new code.

## CI Configuration

```yaml
test:
  services:
    docker:
      image: docker:dind
  script:
    - cargo test -p octofhir-server --test '*'
  allow_failure: false
```

## Adding New Tests

1. Create test file in `tests/` directory
2. Follow naming convention: `test_{feature}.rs`
3. Use testcontainers for database
4. Include setup, execution, verification, cleanup
5. Add to CI if not ignored
6. Document any special requirements
