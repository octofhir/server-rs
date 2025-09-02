use octofhir_core::{CoreError, Result, ResourceType, ResourceEnvelope};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionOperation {
    Create {
        resource_type: ResourceType,
        resource: ResourceEnvelope,
    },
    Update {
        resource_type: ResourceType,
        id: String,
        resource: ResourceEnvelope,
    },
    Delete {
        resource_type: ResourceType,
        id: String,
    },
    Read {
        resource_type: ResourceType,
        id: String,
    },
}

impl TransactionOperation {
    pub fn resource_type(&self) -> &ResourceType {
        match self {
            Self::Create { resource_type, .. } => resource_type,
            Self::Update { resource_type, .. } => resource_type,
            Self::Delete { resource_type, .. } => resource_type,
            Self::Read { resource_type, .. } => resource_type,
        }
    }

    pub fn resource_id(&self) -> Option<&str> {
        match self {
            Self::Create { resource, .. } => Some(&resource.id),
            Self::Update { id, .. } => Some(id),
            Self::Delete { id, .. } => Some(id),
            Self::Read { id, .. } => Some(id),
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::Read { .. })
    }

    pub fn is_write_operation(&self) -> bool {
        !self.is_read_only()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionOperationResult {
    pub operation_id: Uuid,
    pub operation: TransactionOperation,
    pub success: bool,
    pub result: Option<ResourceEnvelope>,
    pub error: Option<String>,
}

impl TransactionOperationResult {
    pub fn success(operation_id: Uuid, operation: TransactionOperation, result: Option<ResourceEnvelope>) -> Self {
        Self {
            operation_id,
            operation,
            success: true,
            result,
            error: None,
        }
    }

    pub fn failure(operation_id: Uuid, operation: TransactionOperation, error: String) -> Self {
        Self {
            operation_id,
            operation,
            success: false,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    Building,
    Executing,
    Committed,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: Uuid,
    pub operations: Vec<(Uuid, TransactionOperation)>,
    pub results: Vec<TransactionOperationResult>,
    pub state: TransactionState,
    pub created_at: octofhir_core::FhirDateTime,
    pub executed_at: Option<octofhir_core::FhirDateTime>,
    pub completed_at: Option<octofhir_core::FhirDateTime>,
    pub rollback_snapshots: HashMap<String, Option<ResourceEnvelope>>, // key: resource_type/id
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            operations: Vec::new(),
            results: Vec::new(),
            state: TransactionState::Building,
            created_at: octofhir_core::time::now_utc(),
            executed_at: None,
            completed_at: None,
            rollback_snapshots: HashMap::new(),
        }
    }

    pub fn add_operation(&mut self, operation: TransactionOperation) -> Result<Uuid> {
        if self.state != TransactionState::Building {
            return Err(CoreError::invalid_resource("Cannot add operations to non-building transaction".to_string()));
        }

        let operation_id = Uuid::new_v4();
        self.operations.push((operation_id, operation));
        Ok(operation_id)
    }

    pub fn create_resource(&mut self, resource_type: ResourceType, resource: ResourceEnvelope) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Create { resource_type, resource })
    }

    pub fn update_resource(&mut self, resource_type: ResourceType, id: String, resource: ResourceEnvelope) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Update { resource_type, id, resource })
    }

    pub fn delete_resource(&mut self, resource_type: ResourceType, id: String) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Delete { resource_type, id })
    }

    pub fn read_resource(&mut self, resource_type: ResourceType, id: String) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Read { resource_type, id })
    }

    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    pub fn write_operation_count(&self) -> usize {
        self.operations.iter().filter(|(_, op)| op.is_write_operation()).count()
    }

    pub fn read_operation_count(&self) -> usize {
        self.operations.iter().filter(|(_, op)| op.is_read_only()).count()
    }

    pub fn can_execute(&self) -> bool {
        matches!(self.state, TransactionState::Building) && !self.operations.is_empty()
    }

    pub fn can_commit(&self) -> bool {
        matches!(self.state, TransactionState::Executing)
    }

    pub fn can_rollback(&self) -> bool {
        matches!(self.state, TransactionState::Executing | TransactionState::Failed)
    }

    pub fn is_completed(&self) -> bool {
        matches!(self.state, TransactionState::Committed | TransactionState::RolledBack)
    }

    pub fn mark_executing(&mut self) {
        if self.state == TransactionState::Building {
            self.state = TransactionState::Executing;
            self.executed_at = Some(octofhir_core::time::now_utc());
        }
    }

    pub fn mark_committed(&mut self) {
        if self.state == TransactionState::Executing {
            self.state = TransactionState::Committed;
            self.completed_at = Some(octofhir_core::time::now_utc());
        }
    }

    pub fn mark_rolled_back(&mut self) {
        if matches!(self.state, TransactionState::Executing | TransactionState::Failed) {
            self.state = TransactionState::RolledBack;
            self.completed_at = Some(octofhir_core::time::now_utc());
        }
    }

    pub fn mark_failed(&mut self) {
        if self.state == TransactionState::Executing {
            self.state = TransactionState::Failed;
        }
    }

    pub fn add_result(&mut self, result: TransactionOperationResult) {
        self.results.push(result);
    }

    pub fn add_rollback_snapshot(&mut self, key: String, snapshot: Option<ResourceEnvelope>) {
        self.rollback_snapshots.insert(key, snapshot);
    }

    pub fn get_rollback_snapshot(&self, key: &str) -> Option<&Option<ResourceEnvelope>> {
        self.rollback_snapshots.get(key)
    }

    pub fn has_failures(&self) -> bool {
        self.results.iter().any(|result| !result.success)
    }

    pub fn success_count(&self) -> usize {
        self.results.iter().filter(|result| result.success).count()
    }

    pub fn failure_count(&self) -> usize {
        self.results.iter().filter(|result| !result.success).count()
    }

    pub fn get_failed_operations(&self) -> Vec<&TransactionOperationResult> {
        self.results.iter().filter(|result| !result.success).collect()
    }

    pub fn duration_ms(&self) -> Option<u64> {
        match (self.executed_at.as_ref(), self.completed_at.as_ref()) {
            (Some(start), Some(end)) => {
                let duration = end.0 - start.0;
                Some(duration.whole_milliseconds() as u64)
            }
            _ => None,
        }
    }

    pub fn clear_operations(&mut self) {
        if self.state == TransactionState::Building {
            self.operations.clear();
        }
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionStats {
    pub total_transactions: u64,
    pub committed_transactions: u64,
    pub rolled_back_transactions: u64,
    pub failed_transactions: u64,
    pub average_duration_ms: f64,
    pub total_operations: u64,
    pub average_operations_per_transaction: f64,
}

impl TransactionStats {
    pub fn new() -> Self {
        Self {
            total_transactions: 0,
            committed_transactions: 0,
            rolled_back_transactions: 0,
            failed_transactions: 0,
            average_duration_ms: 0.0,
            total_operations: 0,
            average_operations_per_transaction: 0.0,
        }
    }

    pub fn record_transaction(&mut self, transaction: &Transaction) {
        self.total_transactions += 1;
        self.total_operations += transaction.operation_count() as u64;

        match transaction.state {
            TransactionState::Committed => self.committed_transactions += 1,
            TransactionState::RolledBack => self.rolled_back_transactions += 1,
            TransactionState::Failed => self.failed_transactions += 1,
            _ => {}
        }

        self.recalculate_averages();
    }

    fn recalculate_averages(&mut self) {
        if self.total_transactions > 0 {
            self.average_operations_per_transaction = 
                self.total_operations as f64 / self.total_transactions as f64;
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            return 0.0;
        }
        self.committed_transactions as f64 / self.total_transactions as f64
    }

    pub fn failure_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            return 0.0;
        }
        (self.rolled_back_transactions + self.failed_transactions) as f64 / self.total_transactions as f64
    }
}

impl Default for TransactionStats {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
pub trait TransactionManager {
    async fn begin_transaction(&mut self) -> Result<Transaction>;
    async fn execute_transaction(&mut self, transaction: &mut Transaction) -> Result<()>;
    async fn commit_transaction(&mut self, transaction: &mut Transaction) -> Result<()>;
    async fn rollback_transaction(&mut self, transaction: &mut Transaction) -> Result<()>;
    async fn abort_transaction(&mut self, transaction: &mut Transaction) -> Result<()>;
    fn get_transaction_stats(&self) -> &TransactionStats;
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::{ResourceType, ResourceEnvelope, ResourceStatus};

    fn create_test_resource(id: &str) -> ResourceEnvelope {
        ResourceEnvelope::new(id.to_string(), ResourceType::Patient)
            .with_status(ResourceStatus::Active)
    }

    #[test]
    fn test_transaction_new() {
        let tx = Transaction::new();
        assert!(tx.id != Uuid::nil());
        assert!(tx.operations.is_empty());
        assert!(tx.results.is_empty());
        assert_eq!(tx.state, TransactionState::Building);
        assert!(tx.executed_at.is_none());
        assert!(tx.completed_at.is_none());
        assert!(tx.rollback_snapshots.is_empty());
    }

    #[test]
    fn test_transaction_add_operations() {
        let mut tx = Transaction::new();
        let resource = create_test_resource("patient-123");

        let create_id = tx.create_resource(ResourceType::Patient, resource.clone()).unwrap();
        let update_id = tx.update_resource(ResourceType::Patient, "patient-456".to_string(), resource.clone()).unwrap();
        let delete_id = tx.delete_resource(ResourceType::Patient, "patient-789".to_string()).unwrap();
        let read_id = tx.read_resource(ResourceType::Patient, "patient-abc".to_string()).unwrap();

        assert_eq!(tx.operation_count(), 4);
        assert_eq!(tx.write_operation_count(), 3);
        assert_eq!(tx.read_operation_count(), 1);

        assert!(create_id != update_id);
        assert!(update_id != delete_id);
        assert!(delete_id != read_id);
    }

    #[test]
    fn test_transaction_operation_resource_type() {
        let resource = create_test_resource("test-123");
        let create_op = TransactionOperation::Create {
            resource_type: ResourceType::Patient,
            resource: resource.clone(),
        };
        let update_op = TransactionOperation::Update {
            resource_type: ResourceType::Organization,
            id: "org-123".to_string(),
            resource,
        };
        let delete_op = TransactionOperation::Delete {
            resource_type: ResourceType::Observation,
            id: "obs-123".to_string(),
        };
        let read_op = TransactionOperation::Read {
            resource_type: ResourceType::Practitioner,
            id: "prac-123".to_string(),
        };

        assert_eq!(create_op.resource_type(), &ResourceType::Patient);
        assert_eq!(update_op.resource_type(), &ResourceType::Organization);
        assert_eq!(delete_op.resource_type(), &ResourceType::Observation);
        assert_eq!(read_op.resource_type(), &ResourceType::Practitioner);
    }

    #[test]
    fn test_transaction_operation_resource_id() {
        let resource = create_test_resource("patient-123");
        let create_op = TransactionOperation::Create {
            resource_type: ResourceType::Patient,
            resource,
        };
        let update_op = TransactionOperation::Update {
            resource_type: ResourceType::Patient,
            id: "patient-456".to_string(),
            resource: create_test_resource("patient-456"),
        };
        let delete_op = TransactionOperation::Delete {
            resource_type: ResourceType::Patient,
            id: "patient-789".to_string(),
        };

        assert_eq!(create_op.resource_id(), Some("patient-123"));
        assert_eq!(update_op.resource_id(), Some("patient-456"));
        assert_eq!(delete_op.resource_id(), Some("patient-789"));
    }

    #[test]
    fn test_transaction_operation_read_only() {
        let resource = create_test_resource("test-123");
        let create_op = TransactionOperation::Create {
            resource_type: ResourceType::Patient,
            resource: resource.clone(),
        };
        let read_op = TransactionOperation::Read {
            resource_type: ResourceType::Patient,
            id: "test-123".to_string(),
        };

        assert!(!create_op.is_read_only());
        assert!(create_op.is_write_operation());
        assert!(read_op.is_read_only());
        assert!(!read_op.is_write_operation());
    }

    #[test]
    fn test_transaction_states() {
        let mut tx = Transaction::new();
        assert_eq!(tx.state, TransactionState::Building);
        assert!(!tx.can_execute()); // Empty transaction cannot execute
        assert!(!tx.can_commit());
        assert!(!tx.can_rollback());
        assert!(!tx.is_completed());

        // Add an operation to allow execution
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        assert!(tx.can_execute()); // Now transaction can execute

        tx.mark_executing();
        assert_eq!(tx.state, TransactionState::Executing);
        assert!(!tx.can_execute());
        assert!(tx.can_commit());
        assert!(tx.can_rollback());
        assert!(!tx.is_completed());
        assert!(tx.executed_at.is_some());

        tx.mark_committed();
        assert_eq!(tx.state, TransactionState::Committed);
        assert!(!tx.can_execute());
        assert!(!tx.can_commit());
        assert!(!tx.can_rollback());
        assert!(tx.is_completed());
        assert!(tx.completed_at.is_some());
    }

    #[test]
    fn test_transaction_rollback_state() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        tx.mark_executing();

        tx.mark_rolled_back();
        assert_eq!(tx.state, TransactionState::RolledBack);
        assert!(!tx.can_execute());
        assert!(!tx.can_commit());
        assert!(!tx.can_rollback());
        assert!(tx.is_completed());
        assert!(tx.completed_at.is_some());
    }

    #[test]
    fn test_transaction_failed_state() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        tx.mark_executing();

        tx.mark_failed();
        assert_eq!(tx.state, TransactionState::Failed);
        assert!(tx.can_rollback());
        assert!(!tx.can_commit());
    }

    #[test]
    fn test_transaction_cannot_add_operations_after_building() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        tx.mark_executing();

        let result = tx.create_resource(ResourceType::Patient, create_test_resource("test-456"));
        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_results() {
        let mut tx = Transaction::new();
        let op_id = Uuid::new_v4();
        let operation = TransactionOperation::Read {
            resource_type: ResourceType::Patient,
            id: "test-123".to_string(),
        };

        let success_result = TransactionOperationResult::success(
            op_id,
            operation.clone(),
            Some(create_test_resource("test-123")),
        );
        let failure_result = TransactionOperationResult::failure(
            op_id,
            operation,
            "Resource not found".to_string(),
        );

        tx.add_result(success_result);
        tx.add_result(failure_result);

        assert_eq!(tx.results.len(), 2);
        assert_eq!(tx.success_count(), 1);
        assert_eq!(tx.failure_count(), 1);
        assert!(tx.has_failures());
        assert_eq!(tx.get_failed_operations().len(), 1);
    }

    #[test]
    fn test_transaction_rollback_snapshots() {
        let mut tx = Transaction::new();
        let resource = create_test_resource("test-123");
        let key = "Patient/test-123".to_string();

        tx.add_rollback_snapshot(key.clone(), Some(resource.clone()));
        
        let snapshot = tx.get_rollback_snapshot(&key);
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().as_ref().unwrap().id, "test-123");

        // Test snapshot for deleted resource (None)
        let deleted_key = "Patient/deleted-123".to_string();
        tx.add_rollback_snapshot(deleted_key.clone(), None);
        let deleted_snapshot = tx.get_rollback_snapshot(&deleted_key);
        assert!(deleted_snapshot.is_some());
        assert!(deleted_snapshot.unwrap().is_none());
    }

    #[test]
    fn test_transaction_duration() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        
        assert!(tx.duration_ms().is_none());

        tx.mark_executing();
        assert!(tx.duration_ms().is_none());

        std::thread::sleep(std::time::Duration::from_millis(10));
        tx.mark_committed();
        
        let duration = tx.duration_ms();
        assert!(duration.is_some());
        assert!(duration.unwrap() >= 10);
    }

    #[test]
    fn test_transaction_clear_operations() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        assert_eq!(tx.operation_count(), 1);

        tx.clear_operations();
        assert_eq!(tx.operation_count(), 0);

        // Should not clear after state change
        tx.create_resource(ResourceType::Patient, create_test_resource("test-456")).unwrap();
        tx.mark_executing();
        tx.clear_operations();
        assert_eq!(tx.operation_count(), 1);
    }

    #[test]
    fn test_transaction_operation_result_constructors() {
        let op_id = Uuid::new_v4();
        let operation = TransactionOperation::Read {
            resource_type: ResourceType::Patient,
            id: "test-123".to_string(),
        };
        let resource = create_test_resource("test-123");

        let success_result = TransactionOperationResult::success(
            op_id,
            operation.clone(),
            Some(resource),
        );
        assert!(success_result.success);
        assert!(success_result.result.is_some());
        assert!(success_result.error.is_none());

        let failure_result = TransactionOperationResult::failure(
            op_id,
            operation,
            "Error message".to_string(),
        );
        assert!(!failure_result.success);
        assert!(failure_result.result.is_none());
        assert_eq!(failure_result.error.as_ref().unwrap(), "Error message");
    }

    #[test]
    fn test_transaction_stats() {
        let mut stats = TransactionStats::new();
        assert_eq!(stats.total_transactions, 0);
        assert_eq!(stats.success_rate(), 0.0);
        assert_eq!(stats.failure_rate(), 0.0);

        let mut committed_tx = Transaction::new();
        committed_tx.create_resource(ResourceType::Patient, create_test_resource("test-1")).unwrap();
        committed_tx.create_resource(ResourceType::Patient, create_test_resource("test-2")).unwrap();
        committed_tx.mark_executing();
        committed_tx.mark_committed();

        let mut rolled_back_tx = Transaction::new();
        rolled_back_tx.create_resource(ResourceType::Patient, create_test_resource("test-3")).unwrap();
        rolled_back_tx.mark_executing();
        rolled_back_tx.mark_rolled_back();

        stats.record_transaction(&committed_tx);
        stats.record_transaction(&rolled_back_tx);

        assert_eq!(stats.total_transactions, 2);
        assert_eq!(stats.committed_transactions, 1);
        assert_eq!(stats.rolled_back_transactions, 1);
        assert_eq!(stats.total_operations, 3);
        assert_eq!(stats.average_operations_per_transaction, 1.5);
        assert_eq!(stats.success_rate(), 0.5);
        assert_eq!(stats.failure_rate(), 0.5);
    }

    #[test]
    fn test_transaction_stats_default() {
        let stats = TransactionStats::default();
        assert_eq!(stats.total_transactions, 0);
        assert_eq!(stats.success_rate(), 0.0);
    }

    #[test]
    fn test_transaction_default() {
        let tx = Transaction::default();
        assert_eq!(tx.state, TransactionState::Building);
        assert!(tx.operations.is_empty());
    }

    #[test]
    fn test_transaction_cannot_execute_empty() {
        let tx = Transaction::new();
        assert!(!tx.can_execute()); // No operations
        
        let mut tx_with_ops = Transaction::new();
        tx_with_ops.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        assert!(tx_with_ops.can_execute()); // Has operations
    }

    #[test]
    fn test_transaction_serialization() {
        let mut tx = Transaction::new();
        tx.create_resource(ResourceType::Patient, create_test_resource("test-123")).unwrap();
        
        let json = serde_json::to_string(&tx).unwrap();
        let deserialized: Transaction = serde_json::from_str(&json).unwrap();
        
        assert_eq!(tx.id, deserialized.id);
        assert_eq!(tx.state, deserialized.state);
        assert_eq!(tx.operations.len(), deserialized.operations.len());
    }

    #[test]
    fn test_transaction_stats_serialization() {
        let stats = TransactionStats::new();
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: TransactionStats = serde_json::from_str(&json).unwrap();
        
        assert_eq!(stats.total_transactions, deserialized.total_transactions);
        assert_eq!(stats.committed_transactions, deserialized.committed_transactions);
    }
}