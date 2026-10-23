//! Cluster-wide lock that serializes startup bootstrap across server instances.
//!
//! Several instances may start against one database at once (rolling deploys,
//! horizontal scaling). Bootstrap steps — package install, resource table DDL,
//! auth seeding — check for existing state and then write it, which races
//! between instances. Holding this lock makes the first instance do the work
//! and the others find it done.

use sqlx_core::connection::Connection;
use sqlx_core::query::query;
use sqlx_postgres::{PgConnection, PgPool};
use tracing::debug;

use crate::error::Result;

/// Advisory-lock key reserved for startup bootstrap ("octofhir" in ASCII).
const BOOTSTRAP_LOCK_KEY: i64 = 0x6f63_746f_6668_6972;

/// Holds the bootstrap advisory lock until dropped.
///
/// The lock is session-level and lives on a dedicated connection outside the
/// pool, so it occupies no pool slot needed by the bootstrap work itself.
/// Dropping the guard closes that connection, which releases the lock; a
/// crashed instance releases it the same way when its socket closes.
pub struct BootstrapLock {
    connection: Option<PgConnection>,
}

impl BootstrapLock {
    /// Waits until no other instance is bootstrapping, then takes the lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be opened or the lock query fails.
    pub async fn acquire(pool: &PgPool) -> Result<Self> {
        let mut connection = PgConnection::connect_with(&pool.connect_options()).await?;
        debug!("Waiting for the bootstrap lock");
        query("SELECT pg_advisory_lock($1)")
            .bind(BOOTSTRAP_LOCK_KEY)
            .execute(&mut connection)
            .await?;
        debug!("Bootstrap lock acquired");
        Ok(Self {
            connection: Some(connection),
        })
    }
}

impl Drop for BootstrapLock {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        // Close gracefully when a runtime is available; otherwise dropping the
        // socket ends the session and the server releases the lock anyway.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = connection.close().await;
                debug!("Bootstrap lock released");
            });
        }
    }
}
