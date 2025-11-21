//! History query implementations.
//!
//! This module contains the SQL queries for FHIR history operations.

use sqlx_postgres::PgPool;

/// History query executor for FHIR resources.
#[derive(Debug, Clone)]
pub struct HistoryQueries {
    pool: PgPool,
}

impl HistoryQueries {
    /// Creates a new `HistoryQueries` with the given connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    // History query tests will be added when implementation is complete.
}
