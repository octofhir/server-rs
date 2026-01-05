//! Connection pool management for the PostgreSQL storage backend.

use std::time::Duration;

use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use tracing::{debug, info, instrument};

use crate::config::PostgresConfig;
use crate::error::{PostgresError, Result};

/// Type alias for PostgreSQL pool options.
pub type PgPoolOptions = PoolOptions<Postgres>;

/// Creates a new PostgreSQL connection pool from the given configuration.
#[instrument(skip(config), fields(url = %mask_password(&config.url)))]
pub async fn create_pool(config: &PostgresConfig) -> Result<PgPool> {
    info!(
        pool_size = config.pool_size,
        min_connections = ?config.min_connections,
        connect_timeout_ms = config.connect_timeout_ms,
        max_lifetime_secs = ?config.max_lifetime_secs,
        "Creating PostgreSQL connection pool"
    );

    let min_connections = config
        .min_connections
        .unwrap_or(config.pool_size / 4)
        .max(1);

    let max_lifetime_secs = config.max_lifetime_secs.unwrap_or(1800);

    let mut options = PgPoolOptions::new()
        .max_connections(config.pool_size)
        .min_connections(min_connections)
        .acquire_timeout(Duration::from_millis(config.connect_timeout_ms))
        .max_lifetime(Duration::from_secs(max_lifetime_secs))
        .test_before_acquire(false);

    if let Some(idle_timeout) = config.idle_timeout_ms {
        options = options.idle_timeout(Duration::from_millis(idle_timeout));
    }

    let pool = options.connect(&config.url).await?;

    debug!("PostgreSQL connection pool created successfully");

    Ok(pool)
}

/// Tests the connection to the database.
#[allow(dead_code)]
#[instrument(skip(pool))]
pub async fn test_connection(pool: &PgPool) -> Result<()> {
    sqlx_core::query::query("SELECT 1")
        .execute(pool)
        .await
        .map_err(PostgresError::from)?;

    debug!("Database connection test successful");

    Ok(())
}

/// Masks the password in a database URL for logging.
fn mask_password(url: &str) -> String {
    // Simple password masking for logging
    if let Some(at_pos) = url.find('@')
        && let Some(colon_pos) = url[..at_pos].rfind(':')
    {
        let scheme_end = url.find("://").map(|p| p + 3).unwrap_or(0);
        if colon_pos > scheme_end {
            return format!("{}:****{}", &url[..colon_pos], &url[at_pos..]);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_password() {
        assert_eq!(
            mask_password("postgres://user:secret@localhost/db"),
            "postgres://user:****@localhost/db"
        );

        assert_eq!(
            mask_password("postgres://localhost/db"),
            "postgres://localhost/db"
        );

        assert_eq!(
            mask_password("postgres://user@localhost/db"),
            "postgres://user@localhost/db"
        );
    }
}
