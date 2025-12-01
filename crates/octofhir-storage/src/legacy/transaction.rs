//! Transaction types for atomic FHIR operations.

use octofhir_core::{CoreError, ResourceEnvelope, ResourceType, Result};
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
    pub fn success(
        operation_id: Uuid,
        operation: TransactionOperation,
        result: Option<ResourceEnvelope>,
    ) -> Self {
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
    pub rollback_snapshots: HashMap<String, Option<ResourceEnvelope>>,
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
            return Err(CoreError::invalid_resource(
                "Cannot add operations to non-building transaction".to_string(),
            ));
        }

        let operation_id = Uuid::new_v4();
        self.operations.push((operation_id, operation));
        Ok(operation_id)
    }

    pub fn create_resource(
        &mut self,
        resource_type: ResourceType,
        resource: ResourceEnvelope,
    ) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Create {
            resource_type,
            resource,
        })
    }

    pub fn update_resource(
        &mut self,
        resource_type: ResourceType,
        id: String,
        resource: ResourceEnvelope,
    ) -> Result<Uuid> {
        self.add_operation(TransactionOperation::Update {
            resource_type,
            id,
            resource,
        })
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
        self.operations
            .iter()
            .filter(|(_, op)| op.is_write_operation())
            .count()
    }

    pub fn read_operation_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|(_, op)| op.is_read_only())
            .count()
    }

    pub fn can_execute(&self) -> bool {
        matches!(self.state, TransactionState::Building) && !self.operations.is_empty()
    }

    pub fn can_commit(&self) -> bool {
        matches!(self.state, TransactionState::Executing)
    }

    pub fn can_rollback(&self) -> bool {
        matches!(
            self.state,
            TransactionState::Executing | TransactionState::Failed
        )
    }

    pub fn is_completed(&self) -> bool {
        matches!(
            self.state,
            TransactionState::Committed | TransactionState::RolledBack
        )
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
        if matches!(
            self.state,
            TransactionState::Executing | TransactionState::Failed
        ) {
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
        self.results
            .iter()
            .filter(|result| !result.success)
            .collect()
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
        (self.rolled_back_transactions + self.failed_transactions) as f64
            / self.total_transactions as f64
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
    fn get_transaction_stats(&self) -> TransactionStats;
}
