//! Upgrade the database created by the published v0.0.5 image without rewriting checksums.

use octofhir_db_postgres::migrations;
use sqlx_core::query_as::query_as;
use testcontainers::{GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
async fn published_v0_0_5_database_upgrades_without_checksum_changes() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let address = postgres.get_bridge_ip_address().await.unwrap();
    let config = format!(
        r#"[storage.postgres]
host = "{address}"
port = 5432
user = "postgres"
password = "postgres"
database = "postgres"
[packages]
load = []
[otel]
enabled = false
[logging]
level = "info"
"#
    );
    // Pin the published multi-platform image, rather than a mutable latest tag.
    let old = GenericImage::new(
        "ghcr.io/octofhir/octofhir-server",
        "0.0.5@sha256:92151cd0c176b85674b67397c344edaeb68a2337bc3c163423537a3d423d8b2d",
    )
    .with_wait_for(WaitFor::message_on_stdout(
        "Database migrations completed successfully",
    ))
    .with_copy_to("/opt/octofhir/config/upgrade.toml", config.into_bytes())
    .with_env_var("OCTOFHIR_CONFIG", "/opt/octofhir/config/upgrade.toml")
    .with_startup_timeout(std::time::Duration::from_secs(120))
    .start()
    .await
    .unwrap();
    old.stop().await.unwrap();

    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = sqlx_postgres::PgPoolOptions::new()
        .connect(&url)
        .await
        .unwrap();
    let before: Vec<(i64, Vec<u8>, bool)> =
        query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(before.len(), 7);
    assert!(
        before
            .iter()
            .all(|(_, checksum, success)| checksum.len() == 48 && *success)
    );
    assert_eq!(before[0].0, 20241213000001);

    migrations::run(&pool, &url).await.unwrap();
    let after: Vec<(i64, Vec<u8>, bool)> =
        query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        after.starts_with(&before),
        "Startup must preserve published migration checksums"
    );
    migrations::run(&pool, &url).await.unwrap();
    let repeated: Vec<(i64, Vec<u8>, bool)> =
        query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(after, repeated, "Repeated startup must be idempotent");
}
