//! Several server instances booting against one fresh database at the same
//! time — the rolling-deploy / horizontal-scaling case — must all come up and
//! seed shared state exactly once.

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::config::{AdminUserConfig, BackendClientConfig};
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use sqlx_core::query_scalar::query_scalar;
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const INSTANCES: usize = 3;

fn instance_config(postgres_url: &str) -> AppConfig {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 10,
        connect_timeout_ms: 60000,
        ..Default::default()
    });
    config.bootstrap.admin_user = Some(AdminUserConfig {
        username: "admin".into(),
        password: "admin123".into(),
        email: None,
        fhir_user: None,
    });
    config.bootstrap.backend_client = Some(BackendClientConfig {
        client_id: "multi-instance-backend".into(),
        ..serde_json::from_value(serde_json::json!({})).expect("default backend client")
    });
    config
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_instances_boot_against_one_database() {
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = postgres.get_host_port_ipv4(5432).await.expect("get port");
    let postgres_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let boots = (0..INSTANCES).map(|_| {
        let config = instance_config(&postgres_url);
        tokio::spawn(async move {
            let config_manager = Arc::new(
                ConfigurationManager::builder()
                    .build()
                    .await
                    .expect("build config manager"),
            );
            build_app(&config, config_manager).await.map(|_| ())
        })
    });
    for (instance, boot) in futures::future::join_all(boots)
        .await
        .into_iter()
        .enumerate()
    {
        boot.expect("boot task")
            .unwrap_or_else(|error| panic!("instance {instance} failed to boot: {error:#}"));
    }

    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&postgres_url)
        .await
        .expect("connect");
    for (sql, what) in [
        (
            r#"SELECT COUNT(*) FROM "user" WHERE resource->>'username' = 'admin'"#,
            "admin users",
        ),
        (
            "SELECT COUNT(*) FROM client WHERE resource->>'clientId' = 'multi-instance-backend'",
            "backend clients",
        ),
    ] {
        let count: i64 = query_scalar(sqlx_core::sql_str::AssertSqlSafe(sql))
            .fetch_one(&pool)
            .await
            .expect(what);
        assert_eq!(count, 1, "{what} must be seeded exactly once");
    }
}
