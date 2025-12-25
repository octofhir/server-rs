//! Integration tests for FHIR resolver using testcontainers.
//!
//! These tests spin up a PostgreSQL container and test the FhirResolver
//! with real schema loading from the FHIR packages.
//!
//! **Requirements:**
//! - Docker running
//! - FHIR packages installed in `.fhir/` directory
//!
//! Run with: cargo test -p octofhir-server --test fhir_resolver_integration -- --ignored

use octofhir_fhir_model::provider::FhirVersion;
use octofhir_server::lsp::{FhirResolver, LoadingState};
use octofhir_server::model_provider::OctoFhirModelProvider;
use std::sync::Arc;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

/// Helper to start a PostgreSQL container and return the connection URL
async fn start_postgres() -> (ContainerAsync<Postgres>, sqlx_postgres::PgPool) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container");

    let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );

    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect to postgres");

    (container, pool)
}

/// Helper to create a model provider with the test database
fn create_model_provider(pool: sqlx_postgres::PgPool) -> Arc<OctoFhirModelProvider> {
    Arc::new(OctoFhirModelProvider::new(pool, FhirVersion::R4B, 100))
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages"]
async fn test_lazy_loading_with_database() {
    let (_container, pool) = start_postgres().await;
    let model_provider = create_model_provider(pool);
    let resolver = FhirResolver::with_model_provider(model_provider);

    // Initially nothing is cached
    assert!(!resolver.is_cached("Patient", ""));
    assert_eq!(
        resolver.get_loading_state("Patient", ""),
        LoadingState::NotLoaded
    );

    // Load Patient children
    let children = resolver.get_children("Patient", "").await;

    // After loading, should be cached
    assert!(resolver.is_cached("Patient", ""));
    assert_eq!(
        resolver.get_loading_state("Patient", ""),
        LoadingState::Loaded
    );

    // Children should include common Patient elements
    // Note: actual elements depend on FHIR package being installed
    println!("Loaded {} children for Patient", children.len());
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages"]
async fn test_preload_common_resources() {
    let (_container, pool) = start_postgres().await;
    let model_provider = create_model_provider(pool);
    let resolver = Arc::new(FhirResolver::with_model_provider(model_provider));

    // Preload common resources
    resolver.preload_common_resources().await;

    // Check that Patient is now cached
    assert!(resolver.is_cached("Patient", ""));
    assert!(resolver.is_cached("Observation", ""));

    // Check cache stats
    let (cache_count, state_count) = resolver.cache_stats();
    println!(
        "After preload: {} cache entries, {} state entries",
        cache_count, state_count
    );
    assert!(cache_count >= 10, "Should have preloaded common resources");
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages"]
async fn test_nested_element_loading() {
    let (_container, pool) = start_postgres().await;
    let model_provider = create_model_provider(pool);
    let resolver = FhirResolver::with_model_provider(model_provider);

    // Load Patient.name children (HumanName elements)
    let name_children = resolver.get_children("Patient", "name").await;

    // HumanName should have given, family, etc.
    let has_given = name_children.iter().any(|e| e.name == "given");
    let has_family = name_children.iter().any(|e| e.name == "family");

    if !name_children.is_empty() {
        assert!(
            has_given || has_family,
            "HumanName should have given or family"
        );
    }

    println!("Loaded {} children for Patient.name", name_children.len());
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages"]
async fn test_cache_clear() {
    let (_container, pool) = start_postgres().await;
    let model_provider = create_model_provider(pool);
    let resolver = FhirResolver::with_model_provider(model_provider);

    // Load some data
    let _ = resolver.get_children("Patient", "").await;
    assert!(resolver.is_cached("Patient", ""));

    // Clear cache
    resolver.clear_cache();

    // Should no longer be cached
    assert!(!resolver.is_cached("Patient", ""));
    assert_eq!(
        resolver.get_loading_state("Patient", ""),
        LoadingState::NotLoaded
    );
}
