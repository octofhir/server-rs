use crate::{ResourceType, FhirDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStats {
    #[serde(rename = "resourceType")]
    pub resource_type: ResourceType,
    #[serde(rename = "totalCount")]
    pub total_count: u64,
    #[serde(rename = "activeCount")]
    pub active_count: u64,
    #[serde(rename = "inactiveCount")]
    pub inactive_count: u64,
    #[serde(rename = "draftCount")]
    pub draft_count: u64,
    #[serde(rename = "unknownCount")]
    pub unknown_count: u64,
    #[serde(rename = "lastUpdated")]
    pub last_updated: FhirDateTime,
}

impl ResourceStats {
    pub fn new(resource_type: ResourceType) -> Self {
        Self {
            resource_type,
            total_count: 0,
            active_count: 0,
            inactive_count: 0,
            draft_count: 0,
            unknown_count: 0,
            last_updated: crate::time::now_utc(),
        }
    }

    pub fn increment_total(&mut self) {
        self.total_count += 1;
        self.last_updated = crate::time::now_utc();
    }

    pub fn decrement_total(&mut self) {
        if self.total_count > 0 {
            self.total_count -= 1;
        }
        self.last_updated = crate::time::now_utc();
    }

    pub fn increment_active(&mut self) {
        self.active_count += 1;
        self.last_updated = crate::time::now_utc();
    }

    pub fn decrement_active(&mut self) {
        if self.active_count > 0 {
            self.active_count -= 1;
        }
        self.last_updated = crate::time::now_utc();
    }

    pub fn increment_inactive(&mut self) {
        self.inactive_count += 1;
        self.last_updated = crate::time::now_utc();
    }

    pub fn decrement_inactive(&mut self) {
        if self.inactive_count > 0 {
            self.inactive_count -= 1;
        }
        self.last_updated = crate::time::now_utc();
    }

    pub fn increment_draft(&mut self) {
        self.draft_count += 1;
        self.last_updated = crate::time::now_utc();
    }

    pub fn decrement_draft(&mut self) {
        if self.draft_count > 0 {
            self.draft_count -= 1;
        }
        self.last_updated = crate::time::now_utc();
    }

    pub fn increment_unknown(&mut self) {
        self.unknown_count += 1;
        self.last_updated = crate::time::now_utc();
    }

    pub fn decrement_unknown(&mut self) {
        if self.unknown_count > 0 {
            self.unknown_count -= 1;
        }
        self.last_updated = crate::time::now_utc();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryStats {
    #[serde(rename = "estimatedBytes")]
    pub estimated_bytes: u64,
    #[serde(rename = "resourceCount")]
    pub resource_count: u64,
    #[serde(rename = "averageResourceSize")]
    pub average_resource_size: f64,
    #[serde(rename = "lastUpdated")]
    pub last_updated: FhirDateTime,
}

impl MemoryStats {
    pub fn new() -> Self {
        Self {
            estimated_bytes: 0,
            resource_count: 0,
            average_resource_size: 0.0,
            last_updated: crate::time::now_utc(),
        }
    }

    pub fn update(&mut self, resource_count: u64, estimated_bytes: u64) {
        self.resource_count = resource_count;
        self.estimated_bytes = estimated_bytes;
        self.average_resource_size = if resource_count > 0 {
            estimated_bytes as f64 / resource_count as f64
        } else {
            0.0
        };
        self.last_updated = crate::time::now_utc();
    }

    pub fn add_resource(&mut self, estimated_size: u64) {
        self.resource_count += 1;
        self.estimated_bytes += estimated_size;
        self.average_resource_size = self.estimated_bytes as f64 / self.resource_count as f64;
        self.last_updated = crate::time::now_utc();
    }

    pub fn remove_resource(&mut self, estimated_size: u64) {
        if self.resource_count > 0 {
            self.resource_count -= 1;
        }
        if self.estimated_bytes >= estimated_size {
            self.estimated_bytes -= estimated_size;
        } else {
            self.estimated_bytes = 0;
        }
        self.average_resource_size = if self.resource_count > 0 {
            self.estimated_bytes as f64 / self.resource_count as f64
        } else {
            0.0
        };
        self.last_updated = crate::time::now_utc();
    }

    pub fn memory_usage_mb(&self) -> f64 {
        self.estimated_bytes as f64 / 1_048_576.0
    }

    pub fn memory_usage_kb(&self) -> f64 {
        self.estimated_bytes as f64 / 1_024.0
    }
}

impl Default for MemoryStats {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

impl Default for HealthStatus {
    fn default() -> Self {
        HealthStatus::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheck {
    pub status: HealthStatus,
    pub message: String,
    #[serde(rename = "checkedAt")]
    pub checked_at: FhirDateTime,
    #[serde(rename = "responseTimeMs", skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    pub details: HashMap<String, serde_json::Value>,
}

impl HealthCheck {
    pub fn healthy(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Healthy,
            message: message.into(),
            checked_at: crate::time::now_utc(),
            response_time_ms: None,
            details: HashMap::new(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Warning,
            message: message.into(),
            checked_at: crate::time::now_utc(),
            response_time_ms: None,
            details: HashMap::new(),
        }
    }

    pub fn critical(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Critical,
            message: message.into(),
            checked_at: crate::time::now_utc(),
            response_time_ms: None,
            details: HashMap::new(),
        }
    }

    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Unknown,
            message: message.into(),
            checked_at: crate::time::now_utc(),
            response_time_ms: None,
            details: HashMap::new(),
        }
    }

    pub fn with_response_time(mut self, response_time: Duration) -> Self {
        self.response_time_ms = Some(response_time.as_millis() as u64);
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }

    pub fn with_details(mut self, details: HashMap<String, serde_json::Value>) -> Self {
        self.details = details;
        self
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.status, HealthStatus::Healthy)
    }

    pub fn is_critical(&self) -> bool {
        matches!(self.status, HealthStatus::Critical)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemMetrics {
    #[serde(rename = "resourceStats")]
    pub resource_stats: HashMap<ResourceType, ResourceStats>,
    #[serde(rename = "memoryStats")]
    pub memory_stats: MemoryStats,
    #[serde(rename = "healthChecks")]
    pub health_checks: HashMap<String, HealthCheck>,
    #[serde(rename = "uptimeSeconds")]
    pub uptime_seconds: u64,
    #[serde(rename = "lastUpdated")]
    pub last_updated: FhirDateTime,
}

impl SystemMetrics {
    pub fn new() -> Self {
        Self {
            resource_stats: HashMap::new(),
            memory_stats: MemoryStats::new(),
            health_checks: HashMap::new(),
            uptime_seconds: 0,
            last_updated: crate::time::now_utc(),
        }
    }

    pub fn get_or_create_resource_stats(&mut self, resource_type: ResourceType) -> &mut ResourceStats {
        self.resource_stats
            .entry(resource_type.clone())
            .or_insert_with(|| ResourceStats::new(resource_type))
    }

    pub fn total_resources(&self) -> u64 {
        self.resource_stats.values().map(|stats| stats.total_count).sum()
    }

    pub fn total_active_resources(&self) -> u64 {
        self.resource_stats.values().map(|stats| stats.active_count).sum()
    }

    pub fn add_health_check(&mut self, name: impl Into<String>, check: HealthCheck) {
        self.health_checks.insert(name.into(), check);
        self.last_updated = crate::time::now_utc();
    }

    pub fn remove_health_check(&mut self, name: &str) -> Option<HealthCheck> {
        let result = self.health_checks.remove(name);
        if result.is_some() {
            self.last_updated = crate::time::now_utc();
        }
        result
    }

    pub fn overall_health_status(&self) -> HealthStatus {
        if self.health_checks.is_empty() {
            return HealthStatus::Unknown;
        }

        let has_critical = self.health_checks.values().any(|check| check.is_critical());
        if has_critical {
            return HealthStatus::Critical;
        }

        let has_warning = self.health_checks.values().any(|check| matches!(check.status, HealthStatus::Warning));
        if has_warning {
            return HealthStatus::Warning;
        }

        let all_healthy = self.health_checks.values().all(|check| check.is_healthy());
        if all_healthy {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unknown
        }
    }

    pub fn update_uptime(&mut self, seconds: u64) {
        self.uptime_seconds = seconds;
        self.last_updated = crate::time::now_utc();
    }

    pub fn uptime_duration(&self) -> Duration {
        Duration::from_secs(self.uptime_seconds)
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self::new()
    }
}

pub trait MetricsCollector {
    fn collect_resource_stats(&self) -> HashMap<ResourceType, ResourceStats>;
    fn collect_memory_stats(&self) -> MemoryStats;
    fn perform_health_check(&self, check_name: &str) -> HealthCheck;
    fn collect_system_metrics(&self) -> SystemMetrics;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resource_stats_new() {
        let stats = ResourceStats::new(ResourceType::Patient);
        assert_eq!(stats.resource_type, ResourceType::Patient);
        assert_eq!(stats.total_count, 0);
        assert_eq!(stats.active_count, 0);
        assert_eq!(stats.inactive_count, 0);
        assert_eq!(stats.draft_count, 0);
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_resource_stats_increment_operations() {
        let mut stats = ResourceStats::new(ResourceType::Patient);
        let original_time = stats.last_updated.clone();

        std::thread::sleep(std::time::Duration::from_millis(1));
        stats.increment_total();
        assert_eq!(stats.total_count, 1);
        assert!(stats.last_updated > original_time);

        stats.increment_active();
        assert_eq!(stats.active_count, 1);

        stats.increment_inactive();
        assert_eq!(stats.inactive_count, 1);

        stats.increment_draft();
        assert_eq!(stats.draft_count, 1);

        stats.increment_unknown();
        assert_eq!(stats.unknown_count, 1);
    }

    #[test]
    fn test_resource_stats_decrement_operations() {
        let mut stats = ResourceStats::new(ResourceType::Patient);
        
        stats.increment_total();
        stats.increment_active();
        stats.increment_inactive();
        stats.increment_draft();
        stats.increment_unknown();

        stats.decrement_total();
        assert_eq!(stats.total_count, 0);

        stats.decrement_active();
        assert_eq!(stats.active_count, 0);

        stats.decrement_inactive();
        assert_eq!(stats.inactive_count, 0);

        stats.decrement_draft();
        assert_eq!(stats.draft_count, 0);

        stats.decrement_unknown();
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_resource_stats_decrement_no_underflow() {
        let mut stats = ResourceStats::new(ResourceType::Patient);
        
        stats.decrement_total();
        assert_eq!(stats.total_count, 0);

        stats.decrement_active();
        assert_eq!(stats.active_count, 0);

        stats.decrement_inactive();
        assert_eq!(stats.inactive_count, 0);

        stats.decrement_draft();
        assert_eq!(stats.draft_count, 0);

        stats.decrement_unknown();
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_memory_stats_new() {
        let stats = MemoryStats::new();
        assert_eq!(stats.estimated_bytes, 0);
        assert_eq!(stats.resource_count, 0);
        assert_eq!(stats.average_resource_size, 0.0);
    }

    #[test]
    fn test_memory_stats_update() {
        let mut stats = MemoryStats::new();
        stats.update(100, 1024);
        
        assert_eq!(stats.resource_count, 100);
        assert_eq!(stats.estimated_bytes, 1024);
        assert_eq!(stats.average_resource_size, 10.24);
    }

    #[test]
    fn test_memory_stats_add_resource() {
        let mut stats = MemoryStats::new();
        stats.add_resource(512);
        
        assert_eq!(stats.resource_count, 1);
        assert_eq!(stats.estimated_bytes, 512);
        assert_eq!(stats.average_resource_size, 512.0);

        stats.add_resource(256);
        assert_eq!(stats.resource_count, 2);
        assert_eq!(stats.estimated_bytes, 768);
        assert_eq!(stats.average_resource_size, 384.0);
    }

    #[test]
    fn test_memory_stats_remove_resource() {
        let mut stats = MemoryStats::new();
        stats.add_resource(512);
        stats.add_resource(256);
        
        stats.remove_resource(256);
        assert_eq!(stats.resource_count, 1);
        assert_eq!(stats.estimated_bytes, 512);
        assert_eq!(stats.average_resource_size, 512.0);

        stats.remove_resource(512);
        assert_eq!(stats.resource_count, 0);
        assert_eq!(stats.estimated_bytes, 0);
        assert_eq!(stats.average_resource_size, 0.0);
    }

    #[test]
    fn test_memory_stats_remove_resource_no_underflow() {
        let mut stats = MemoryStats::new();
        stats.remove_resource(100);
        
        assert_eq!(stats.resource_count, 0);
        assert_eq!(stats.estimated_bytes, 0);

        stats.add_resource(50);
        stats.remove_resource(100);
        assert_eq!(stats.estimated_bytes, 0);
    }

    #[test]
    fn test_memory_stats_conversions() {
        let mut stats = MemoryStats::new();
        stats.update(1, 2_097_152); // 2 MB

        assert_eq!(stats.memory_usage_mb(), 2.0);
        assert_eq!(stats.memory_usage_kb(), 2048.0);
    }

    #[test]
    fn test_health_check_constructors() {
        let healthy = HealthCheck::healthy("All systems operational");
        assert_eq!(healthy.status, HealthStatus::Healthy);
        assert_eq!(healthy.message, "All systems operational");

        let warning = HealthCheck::warning("High memory usage");
        assert_eq!(warning.status, HealthStatus::Warning);

        let critical = HealthCheck::critical("Database connection failed");
        assert_eq!(critical.status, HealthStatus::Critical);

        let unknown = HealthCheck::unknown("Cannot determine status");
        assert_eq!(unknown.status, HealthStatus::Unknown);
    }

    #[test]
    fn test_health_check_with_response_time() {
        let duration = Duration::from_millis(150);
        let check = HealthCheck::healthy("OK").with_response_time(duration);
        assert_eq!(check.response_time_ms, Some(150));
    }

    #[test]
    fn test_health_check_with_details() {
        let check = HealthCheck::healthy("OK")
            .with_detail("cpu_usage", json!(75.5))
            .with_detail("memory_usage", json!("512MB"));
        
        assert_eq!(check.details.len(), 2);
        assert_eq!(check.details["cpu_usage"], json!(75.5));
        assert_eq!(check.details["memory_usage"], json!("512MB"));
    }

    #[test]
    fn test_health_check_status_methods() {
        let healthy = HealthCheck::healthy("OK");
        assert!(healthy.is_healthy());
        assert!(!healthy.is_critical());

        let critical = HealthCheck::critical("Error");
        assert!(!critical.is_healthy());
        assert!(critical.is_critical());
    }

    #[test]
    fn test_system_metrics_new() {
        let metrics = SystemMetrics::new();
        assert!(metrics.resource_stats.is_empty());
        assert_eq!(metrics.memory_stats.resource_count, 0);
        assert!(metrics.health_checks.is_empty());
        assert_eq!(metrics.uptime_seconds, 0);
    }

    #[test]
    fn test_system_metrics_get_or_create_resource_stats() {
        let mut metrics = SystemMetrics::new();
        
        let stats = metrics.get_or_create_resource_stats(ResourceType::Patient);
        assert_eq!(stats.resource_type, ResourceType::Patient);
        
        let stats_again = metrics.get_or_create_resource_stats(ResourceType::Patient);
        assert_eq!(stats_again.resource_type, ResourceType::Patient);
        
        assert_eq!(metrics.resource_stats.len(), 1);
    }

    #[test]
    fn test_system_metrics_totals() {
        let mut metrics = SystemMetrics::new();
        
        let patient_stats = metrics.get_or_create_resource_stats(ResourceType::Patient);
        patient_stats.total_count = 10;
        patient_stats.active_count = 8;
        
        let org_stats = metrics.get_or_create_resource_stats(ResourceType::Organization);
        org_stats.total_count = 5;
        org_stats.active_count = 4;
        
        assert_eq!(metrics.total_resources(), 15);
        assert_eq!(metrics.total_active_resources(), 12);
    }

    #[test]
    fn test_system_metrics_health_checks() {
        let mut metrics = SystemMetrics::new();
        
        metrics.add_health_check("database", HealthCheck::healthy("Connected"));
        metrics.add_health_check("memory", HealthCheck::warning("High usage"));
        
        assert_eq!(metrics.health_checks.len(), 2);
        assert_eq!(metrics.overall_health_status(), HealthStatus::Warning);
        
        let removed = metrics.remove_health_check("memory");
        assert!(removed.is_some());
        assert_eq!(metrics.overall_health_status(), HealthStatus::Healthy);
    }

    #[test]
    fn test_system_metrics_overall_health_status() {
        let mut metrics = SystemMetrics::new();
        
        // Empty checks should return Unknown
        assert_eq!(metrics.overall_health_status(), HealthStatus::Unknown);
        
        // All healthy should return Healthy
        metrics.add_health_check("test1", HealthCheck::healthy("OK"));
        metrics.add_health_check("test2", HealthCheck::healthy("OK"));
        assert_eq!(metrics.overall_health_status(), HealthStatus::Healthy);
        
        // Any warning should return Warning
        metrics.add_health_check("test3", HealthCheck::warning("Warning"));
        assert_eq!(metrics.overall_health_status(), HealthStatus::Warning);
        
        // Any critical should return Critical
        metrics.add_health_check("test4", HealthCheck::critical("Error"));
        assert_eq!(metrics.overall_health_status(), HealthStatus::Critical);
    }

    #[test]
    fn test_system_metrics_uptime() {
        let mut metrics = SystemMetrics::new();
        metrics.update_uptime(3600);
        
        assert_eq!(metrics.uptime_seconds, 3600);
        assert_eq!(metrics.uptime_duration(), Duration::from_secs(3600));
    }

    #[test]
    fn test_resource_stats_serialization() {
        let stats = ResourceStats::new(ResourceType::Patient);
        let json = serde_json::to_value(&stats).unwrap();
        
        assert_eq!(json["resourceType"], "Patient");
        assert_eq!(json["totalCount"], 0);
        assert_eq!(json["activeCount"], 0);
        assert!(json["lastUpdated"].is_string());
    }

    #[test]
    fn test_memory_stats_serialization() {
        let stats = MemoryStats::new();
        let json = serde_json::to_value(&stats).unwrap();
        
        assert_eq!(json["estimatedBytes"], 0);
        assert_eq!(json["resourceCount"], 0);
        assert_eq!(json["averageResourceSize"], 0.0);
        assert!(json["lastUpdated"].is_string());
    }

    #[test]
    fn test_health_check_serialization() {
        let check = HealthCheck::healthy("All good")
            .with_detail("version", json!("1.0.0"));
        let json = serde_json::to_value(&check).unwrap();
        
        assert_eq!(json["status"], "Healthy");
        assert_eq!(json["message"], "All good");
        assert!(json["checkedAt"].is_string());
        assert_eq!(json["details"]["version"], "1.0.0");
    }

    #[test]
    fn test_system_metrics_serialization() {
        let mut metrics = SystemMetrics::new();
        metrics.add_health_check("test", HealthCheck::healthy("OK"));
        
        let json = serde_json::to_value(&metrics).unwrap();
        
        assert!(json["resourceStats"].is_object());
        assert!(json["memoryStats"].is_object());
        assert!(json["healthChecks"].is_object());
        assert_eq!(json["uptimeSeconds"], 0);
        assert!(json["lastUpdated"].is_string());
    }

    #[test]
    fn test_health_status_default() {
        let status = HealthStatus::default();
        assert_eq!(status, HealthStatus::Unknown);
    }

    #[test]
    fn test_memory_stats_default() {
        let stats = MemoryStats::default();
        assert_eq!(stats.estimated_bytes, 0);
        assert_eq!(stats.resource_count, 0);
        assert_eq!(stats.average_resource_size, 0.0);
    }

    #[test]
    fn test_system_metrics_default() {
        let metrics = SystemMetrics::default();
        assert!(metrics.resource_stats.is_empty());
        assert_eq!(metrics.memory_stats.resource_count, 0);
        assert!(metrics.health_checks.is_empty());
        assert_eq!(metrics.uptime_seconds, 0);
    }
}