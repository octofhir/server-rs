// DDL generator for PostgreSQL schema creation
// Converts TableDescriptor into CREATE TABLE, CREATE INDEX statements

use super::types::{ColumnDescriptor, ForeignKeyDescriptor, IndexDescriptor, TableDescriptor};
use std::fmt::Write;

/// Generates PostgreSQL DDL statements from table descriptors
pub struct DdlGenerator {
    /// Schema name (default "public")
    schema: String,
    /// Include IF NOT EXISTS clauses
    if_not_exists: bool,
}

impl Default for DdlGenerator {
    fn default() -> Self {
        Self {
            schema: "public".to_string(),
            if_not_exists: true,
        }
    }
}

impl DdlGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_schema(mut self, schema: String) -> Self {
        self.schema = schema;
        self
    }

    pub fn with_if_not_exists(mut self, value: bool) -> Self {
        self.if_not_exists = value;
        self
    }

    /// Generate complete DDL for a table including indexes and foreign keys
    pub fn generate_table_ddl(&self, table: &TableDescriptor) -> Result<String, std::fmt::Error> {
        let mut ddl = String::new();

        // Generate CREATE TABLE statement
        self.generate_create_table(&mut ddl, table)?;
        ddl.push_str("\n\n");

        // Generate CREATE INDEX statements
        for index in &table.indexes {
            self.generate_create_index(&mut ddl, &table.name, index)?;
            ddl.push_str("\n\n");
        }

        // Generate ALTER TABLE for foreign keys (separate from CREATE TABLE for clarity)
        for fk in &table.foreign_keys {
            self.generate_foreign_key(&mut ddl, &table.name, fk)?;
            ddl.push_str("\n\n");
        }

        Ok(ddl.trim_end().to_string())
    }

    /// Generate CREATE TABLE statement
    fn generate_create_table(
        &self,
        out: &mut String,
        table: &TableDescriptor,
    ) -> Result<(), std::fmt::Error> {
        write!(out, "CREATE TABLE ")?;

        if self.if_not_exists {
            write!(out, "IF NOT EXISTS ")?;
        }

        write!(out, "{}.{} (", self.schema, table.name)?;

        // Generate column definitions
        for (idx, column) in table.columns.iter().enumerate() {
            if idx > 0 {
                write!(out, ",")?;
            }
            write!(out, "\n    ")?;
            self.generate_column_def(out, column)?;
        }

        // Add primary key constraint if multiple columns or named constraint needed
        let pk_columns: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.as_str())
            .collect();

        if pk_columns.len() > 1 {
            write!(out, ",\n    PRIMARY KEY ({})", pk_columns.join(", "))?;
        }

        write!(out, "\n)")?;

        Ok(())
    }

    /// Generate a single column definition
    fn generate_column_def(
        &self,
        out: &mut String,
        column: &ColumnDescriptor,
    ) -> Result<(), std::fmt::Error> {
        write!(out, "{} {}", column.name, column.data_type)?;

        // PRIMARY KEY constraint (for single-column PKs)
        if column.primary_key {
            write!(out, " PRIMARY KEY")?;
        }

        // NOT NULL constraint
        if !column.nullable && !column.primary_key {
            write!(out, " NOT NULL")?;
        }

        // DEFAULT value
        if let Some(default) = &column.default {
            write!(out, " DEFAULT {}", default)?;
        }

        Ok(())
    }

    /// Generate CREATE INDEX statement
    fn generate_create_index(
        &self,
        out: &mut String,
        table_name: &str,
        index: &IndexDescriptor,
    ) -> Result<(), std::fmt::Error> {
        write!(out, "CREATE ")?;

        if index.unique {
            write!(out, "UNIQUE ")?;
        }

        write!(out, "INDEX ")?;

        if self.if_not_exists {
            write!(out, "IF NOT EXISTS ")?;
        }

        write!(
            out,
            "{} ON {}.{} USING {} ({})",
            index.name,
            self.schema,
            table_name,
            index.method,
            index.columns.join(", ")
        )?;

        if let Some(where_clause) = &index.where_clause {
            write!(out, " WHERE {}", where_clause)?;
        }

        Ok(())
    }

    /// Generate ALTER TABLE ADD FOREIGN KEY statement
    fn generate_foreign_key(
        &self,
        out: &mut String,
        table_name: &str,
        fk: &ForeignKeyDescriptor,
    ) -> Result<(), std::fmt::Error> {
        write!(
            out,
            "ALTER TABLE {}.{} ADD CONSTRAINT {} ",
            self.schema, table_name, fk.name
        )?;

        write!(
            out,
            "FOREIGN KEY ({}) REFERENCES {}.{} ({})",
            fk.columns.join(", "),
            self.schema,
            fk.referenced_table,
            fk.referenced_columns.join(", ")
        )?;

        write!(out, " ON DELETE {}", fk.on_delete)?;
        write!(out, " ON UPDATE {}", fk.on_update)?;

        Ok(())
    }

    /// Generate DROP TABLE statement
    pub fn generate_drop_table(&self, table_name: &str) -> Result<String, std::fmt::Error> {
        let mut ddl = String::new();
        write!(
            ddl,
            "DROP TABLE IF EXISTS {}.{} CASCADE",
            self.schema, table_name
        )?;
        Ok(ddl)
    }

    /// Generate complete migration (both create and drop)
    pub fn generate_migration(
        &self,
        table: &TableDescriptor,
    ) -> Result<(String, String), std::fmt::Error> {
        let up = self.generate_table_ddl(table)?;
        let down = self.generate_drop_table(&table.name)?;
        Ok((up, down))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::PostgresType;

    #[test]
    fn test_simple_table_ddl() {
        let generator = DdlGenerator::new();

        let table = TableDescriptor::new("test_table".to_string(), "TestResource".to_string())
            .with_column(
                ColumnDescriptor::new("id".to_string(), PostgresType::Uuid)
                    .primary()
                    .with_default("gen_random_uuid()".to_string()),
            )
            .with_column(
                ColumnDescriptor::new("name".to_string(), PostgresType::Text).not_null(),
            )
            .with_column(ColumnDescriptor::new("data".to_string(), PostgresType::Jsonb));

        let ddl = generator.generate_table_ddl(&table).unwrap();

        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS public.test_table"));
        assert!(ddl.contains("id UUID PRIMARY KEY DEFAULT gen_random_uuid()"));
        assert!(ddl.contains("name TEXT NOT NULL"));
        assert!(ddl.contains("data JSONB"));
    }

    #[test]
    fn test_table_with_indexes() {
        let generator = DdlGenerator::new();

        let table = TableDescriptor::new("patient".to_string(), "Patient".to_string())
            .with_column(
                ColumnDescriptor::new("id".to_string(), PostgresType::Uuid).primary(),
            )
            .with_column(ColumnDescriptor::new("resource".to_string(), PostgresType::Jsonb))
            .with_index(
                IndexDescriptor::new("idx_patient_resource".to_string(), vec!["resource".to_string()])
                    .gin(),
            );

        let ddl = generator.generate_table_ddl(&table).unwrap();

        assert!(ddl.contains("CREATE TABLE"));
        assert!(ddl.contains("CREATE INDEX IF NOT EXISTS idx_patient_resource"));
        assert!(ddl.contains("USING GIN"));
    }

    #[test]
    fn test_drop_table() {
        let generator = DdlGenerator::new();
        let drop_ddl = generator.generate_drop_table("patient").unwrap();

        assert_eq!(drop_ddl, "DROP TABLE IF EXISTS public.patient CASCADE");
    }

    #[test]
    fn test_migration_generation() {
        let generator = DdlGenerator::new();

        let table = TableDescriptor::new("test".to_string(), "Test".to_string())
            .with_column(ColumnDescriptor::new("id".to_string(), PostgresType::Uuid).primary());

        let (up, down) = generator.generate_migration(&table).unwrap();

        assert!(up.contains("CREATE TABLE"));
        assert!(down.contains("DROP TABLE"));
    }

    #[test]
    fn test_custom_schema() {
        let generator = DdlGenerator::new().with_schema("fhir".to_string());

        let table = TableDescriptor::new("patient".to_string(), "Patient".to_string())
            .with_column(ColumnDescriptor::new("id".to_string(), PostgresType::Uuid).primary());

        let ddl = generator.generate_table_ddl(&table).unwrap();

        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS fhir.patient"));
    }

    #[test]
    fn test_if_not_exists_flag() {
        let generator = DdlGenerator::new().with_if_not_exists(false);

        let table = TableDescriptor::new("test".to_string(), "Test".to_string())
            .with_column(ColumnDescriptor::new("id".to_string(), PostgresType::Uuid).primary());

        let ddl = generator.generate_table_ddl(&table).unwrap();

        assert!(ddl.contains("CREATE TABLE public.test"));
        assert!(!ddl.contains("IF NOT EXISTS"));
    }
}