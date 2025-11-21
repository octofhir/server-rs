//! Search query implementations.
//!
//! This module contains the SQL queries for FHIR search operations.

use sqlx_postgres::PgPool;

/// Search query executor for FHIR resources.
#[derive(Debug, Clone)]
pub struct SearchQueries {
    pool: PgPool,
}

impl SearchQueries {
    /// Creates a new `SearchQueries` with the given connection pool.
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
    // Search query tests will be added when implementation is complete.
}
