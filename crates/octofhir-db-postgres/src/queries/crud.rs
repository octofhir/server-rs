//! CRUD (Create, Read, Update, Delete) query implementations.
//!
//! This module contains the SQL queries for basic resource operations.

use sqlx_postgres::PgPool;

/// CRUD query executor for FHIR resources.
#[derive(Debug, Clone)]
pub struct CrudQueries {
    pool: PgPool,
}

impl CrudQueries {
    /// Creates a new `CrudQueries` with the given connection pool.
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
    // CRUD query tests will be added when implementation is complete.
}
