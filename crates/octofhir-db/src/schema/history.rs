// History table descriptors implementing ADR-001 Diff History Strategy
// Supports snapshot-K + JSON Patch/Merge Patch for FHIR resource history

use super::types::{ColumnDescriptor, ForeignKeyAction, ForeignKeyDescriptor, IndexDescriptor, PostgresType, TableDescriptor};

/// Strategy for storing resource history snapshots
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStrategy {
    /// Store full snapshot every K versions
    EveryKVersions(usize),
    /// Store full snapshot for specific versions (e.g., every 10th)
    Selective,
    /// Always store full snapshots (high storage cost)
    Always,
}

impl Default for SnapshotStrategy {
    fn default() -> Self {
        // Default to snapshot every 10 versions per ADR-001
        SnapshotStrategy::EveryKVersions(10)
    }
}

/// Descriptor for a resource history table
#[derive(Debug, Clone)]
pub struct HistoryTableDescriptor {
    /// Base table name (e.g., "patient")
    pub base_table: String,
    /// Resource type
    pub resource_type: String,
    /// Snapshot strategy
    pub strategy: SnapshotStrategy,
}

impl HistoryTableDescriptor {
    pub fn new(base_table: String, resource_type: String) -> Self {
        Self {
            base_table,
            resource_type,
            strategy: SnapshotStrategy::default(),
        }
    }

    pub fn with_strategy(mut self, strategy: SnapshotStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Generate the history table name
    pub fn table_name(&self) -> String {
        format!("{}_history", self.base_table)
    }

    /// Generate the full TableDescriptor for the history table
    ///
    /// Schema per ADR-001:
    /// - id: UUID primary key for history entry
    /// - resource_id: UUID reference to base resource
    /// - version_id: Integer version number
    /// - operation: TEXT (create, update, delete)
    /// - snapshot: JSONB (full resource snapshot, null for non-snapshot versions)
    /// - json_patch: JSONB (JSON Patch operations per RFC 6902)
    /// - merge_patch: JSONB (JSON Merge Patch per RFC 7386)
    /// - author: TEXT (who made the change)
    /// - timestamp: TIMESTAMPTZ (when the change occurred)
    /// - request_id: UUID (correlation ID for auditing)
    pub fn to_table_descriptor(&self) -> TableDescriptor {
        let table_name = self.table_name();

        let mut table = TableDescriptor::new(table_name.clone(), self.resource_type.clone());

        // Primary key
        table.columns.push(
            ColumnDescriptor::new("id".to_string(), PostgresType::Uuid)
                .primary()
                .with_default("gen_random_uuid()".to_string())
                .with_fhir_path(format!("{}.id", self.resource_type)),
        );

        // Reference to base resource
        table.columns.push(
            ColumnDescriptor::new("resource_id".to_string(), PostgresType::Uuid)
                .not_null()
                .with_fhir_path(format!("{}.id", self.resource_type)),
        );

        // Version number
        table.columns.push(
            ColumnDescriptor::new("version_id".to_string(), PostgresType::Integer)
                .not_null()
                .with_fhir_path(format!("{}.meta.versionId", self.resource_type)),
        );

        // Operation type (create, update, delete)
        table.columns.push(
            ColumnDescriptor::new("operation".to_string(), PostgresType::Text)
                .not_null()
        );

        // Full resource snapshot (stored every K versions)
        table.columns.push(
            ColumnDescriptor::new("snapshot".to_string(), PostgresType::Jsonb)
        );

        // JSON Patch operations (RFC 6902)
        table.columns.push(
            ColumnDescriptor::new("json_patch".to_string(), PostgresType::Jsonb)
        );

        // JSON Merge Patch (RFC 7386)
        table.columns.push(
            ColumnDescriptor::new("merge_patch".to_string(), PostgresType::Jsonb)
        );

        // Audit fields
        table.columns.push(
            ColumnDescriptor::new("author".to_string(), PostgresType::Text)
        );

        table.columns.push(
            ColumnDescriptor::new("timestamp".to_string(), PostgresType::Timestamptz)
                .not_null()
                .with_default("CURRENT_TIMESTAMP".to_string())
                .with_fhir_path(format!("{}.meta.lastUpdated", self.resource_type)),
        );

        table.columns.push(
            ColumnDescriptor::new("request_id".to_string(), PostgresType::Uuid)
        );

        // Foreign key to base table
        table.foreign_keys.push(ForeignKeyDescriptor {
            name: format!("fk_{}_history_resource", self.base_table),
            columns: vec!["resource_id".to_string()],
            referenced_table: self.base_table.clone(),
            referenced_columns: vec!["id".to_string()],
            on_delete: ForeignKeyAction::Cascade,
            on_update: ForeignKeyAction::Cascade,
        });

        // Indexes for common queries
        // Index on resource_id for _history/{id} queries
        table.indexes.push(
            IndexDescriptor::new(
                format!("idx_{}_history_resource_id", self.base_table),
                vec!["resource_id".to_string()],
            )
        );

        // Composite index on (resource_id, version_id) for _history/{id}/{vid}
        table.indexes.push(
            IndexDescriptor::new(
                format!("idx_{}_history_resource_version", self.base_table),
                vec!["resource_id".to_string(), "version_id".to_string()],
            ).unique()
        );

        // Index on timestamp for temporal queries
        table.indexes.push(
            IndexDescriptor::new(
                format!("idx_{}_history_timestamp", self.base_table),
                vec!["timestamp".to_string()],
            )
        );

        // GIN index on snapshot JSONB for search within history
        table.indexes.push(
            IndexDescriptor::new(
                format!("idx_{}_history_snapshot_gin", self.base_table),
                vec!["snapshot".to_string()],
            ).gin()
        );

        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_strategy_default() {
        let strategy = SnapshotStrategy::default();
        assert_eq!(strategy, SnapshotStrategy::EveryKVersions(10));
    }

    #[test]
    fn test_history_table_name() {
        let descriptor = HistoryTableDescriptor::new(
            "patient".to_string(),
            "Patient".to_string(),
        );
        assert_eq!(descriptor.table_name(), "patient_history");
    }

    #[test]
    fn test_history_table_descriptor() {
        let descriptor = HistoryTableDescriptor::new(
            "patient".to_string(),
            "Patient".to_string(),
        );
        let table = descriptor.to_table_descriptor();

        assert_eq!(table.name, "patient_history");
        assert_eq!(table.resource_type, "Patient");

        // Check essential columns exist
        assert!(table.columns.iter().any(|c| c.name == "id" && c.primary_key));
        assert!(table.columns.iter().any(|c| c.name == "resource_id"));
        assert!(table.columns.iter().any(|c| c.name == "version_id"));
        assert!(table.columns.iter().any(|c| c.name == "snapshot"));
        assert!(table.columns.iter().any(|c| c.name == "json_patch"));
        assert!(table.columns.iter().any(|c| c.name == "merge_patch"));
        assert!(table.columns.iter().any(|c| c.name == "timestamp"));

        // Check foreign key
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].referenced_table, "patient");

        // Check indexes
        assert!(table.indexes.len() >= 4);
        assert!(table.indexes.iter().any(|i| i.columns.contains(&"resource_id".to_string())));
        assert!(table.indexes.iter().any(|i| i.unique && i.columns.len() == 2));
    }

    #[test]
    fn test_history_with_custom_strategy() {
        let descriptor = HistoryTableDescriptor::new(
            "observation".to_string(),
            "Observation".to_string(),
        ).with_strategy(SnapshotStrategy::EveryKVersions(5));

        assert_eq!(descriptor.strategy, SnapshotStrategy::EveryKVersions(5));
    }
}