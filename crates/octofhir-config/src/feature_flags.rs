//! Feature flags system for dynamic feature toggling
//!
//! Supports various flag types:
//! - Boolean: Simple on/off
//! - Percentage: Gradual rollout based on hash
//! - Tenant-based: Per-tenant enablement
//! - Time-based: Enable during specific time windows

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use time::OffsetDateTime;

/// Context for evaluating feature flags
#[derive(Debug, Clone, Default)]
pub struct FeatureContext {
    /// Tenant ID for multi-tenant deployments
    pub tenant_id: Option<String>,
    /// User ID for user-specific features
    pub user_id: Option<String>,
    /// Request ID for consistent evaluation within a request
    pub request_id: Option<String>,
    /// Custom attributes for advanced targeting
    pub attributes: HashMap<String, String>,
}

impl FeatureContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Create context with tenant ID
    pub fn with_tenant(tenant_id: impl Into<String>) -> Self {
        Self {
            tenant_id: Some(tenant_id.into()),
            ..Default::default()
        }
    }

    /// Add user ID to context
    pub fn user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Add request ID to context
    pub fn request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Add custom attribute
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Get a hash for percentage-based evaluation
    fn hash_for_percentage(&self, flag_name: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        flag_name.hash(&mut hasher);

        // Use the most specific identifier available
        if let Some(ref user_id) = self.user_id {
            user_id.hash(&mut hasher);
        } else if let Some(ref tenant_id) = self.tenant_id {
            tenant_id.hash(&mut hasher);
        } else if let Some(ref request_id) = self.request_id {
            request_id.hash(&mut hasher);
        }

        hasher.finish()
    }
}

/// Type of feature flag
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum FeatureFlagType {
    /// Simple on/off toggle
    #[default]
    Boolean,
    /// Percentage-based rollout (0-100)
    Percentage {
        #[serde(default = "default_percentage")]
        value: u8,
    },
    /// Enable for specific tenants
    TenantBased {
        #[serde(default)]
        allowed_tenants: Vec<String>,
    },
    /// Enable during a time window
    TimeBased {
        #[serde(with = "time::serde::rfc3339::option", default)]
        start: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339::option", default)]
        end: Option<OffsetDateTime>,
    },
}

fn default_percentage() -> u8 {
    0
}

/// A single feature flag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    /// Flag name (e.g., "search.optimization.enabled")
    pub name: String,
    /// Whether the flag is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Flag type for advanced evaluation
    #[serde(default, flatten)]
    pub flag_type: FeatureFlagType,
    /// Description of what this flag controls
    #[serde(default)]
    pub description: Option<String>,
    /// When this flag was last updated
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub updated_at: Option<OffsetDateTime>,
}

impl FeatureFlag {
    /// Create a new boolean feature flag
    pub fn boolean(name: impl Into<String>, enabled: bool) -> Self {
        Self {
            name: name.into(),
            enabled,
            flag_type: FeatureFlagType::Boolean,
            description: None,
            updated_at: Some(OffsetDateTime::now_utc()),
        }
    }

    /// Create a percentage-based feature flag
    pub fn percentage(name: impl Into<String>, percentage: u8) -> Self {
        Self {
            name: name.into(),
            enabled: percentage > 0,
            flag_type: FeatureFlagType::Percentage {
                value: percentage.min(100),
            },
            description: None,
            updated_at: Some(OffsetDateTime::now_utc()),
        }
    }

    /// Create a tenant-based feature flag
    pub fn tenant_based(name: impl Into<String>, allowed_tenants: Vec<String>) -> Self {
        Self {
            name: name.into(),
            enabled: !allowed_tenants.is_empty(),
            flag_type: FeatureFlagType::TenantBased { allowed_tenants },
            description: None,
            updated_at: Some(OffsetDateTime::now_utc()),
        }
    }

    /// Add description to flag
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Evaluate this flag for a given context
    pub fn evaluate(&self, context: &FeatureContext) -> bool {
        if !self.enabled {
            return false;
        }

        match &self.flag_type {
            FeatureFlagType::Boolean => true,

            FeatureFlagType::Percentage { value } => {
                let hash = context.hash_for_percentage(&self.name);
                let bucket = (hash % 100) as u8;
                bucket < *value
            }

            FeatureFlagType::TenantBased { allowed_tenants } => context
                .tenant_id
                .as_ref()
                .is_some_and(|tid| allowed_tenants.contains(tid)),

            FeatureFlagType::TimeBased { start, end } => {
                let now = OffsetDateTime::now_utc();
                let after_start = start.is_none_or(|s| now >= s);
                let before_end = end.is_none_or(|e| now <= e);
                after_start && before_end
            }
        }
    }
}

/// Collection of feature flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(flatten)]
    flags: HashMap<String, FeatureFlag>,
}

impl FeatureFlags {
    /// Create a new empty feature flags collection
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with default built-in flags
    pub fn with_defaults() -> Self {
        let mut flags = Self::new();

        // Search optimization
        flags.set(
            FeatureFlag::boolean("search.optimization.enabled", true)
                .with_description("Enable query optimization in search engine"),
        );

        // Terminology external lookups
        flags.set(
            FeatureFlag::boolean("terminology.external.enabled", true)
                .with_description("Allow external terminology server lookups"),
        );

        // Validation skip header
        flags.set(
            FeatureFlag::boolean("validation.skip.allowed", false)
                .with_description("Allow X-Skip-Validation header"),
        );

        // SMART on FHIR
        flags.set(
            FeatureFlag::boolean("auth.smart_on_fhir.enabled", true)
                .with_description("Enable SMART on FHIR authentication"),
        );

        // Redis cache
        flags.set(
            FeatureFlag::boolean("cache.redis.enabled", false)
                .with_description("Use Redis as cache backend"),
        );

        flags
    }

    /// Set a feature flag
    pub fn set(&mut self, flag: FeatureFlag) {
        self.flags.insert(flag.name.clone(), flag);
    }

    /// Get a feature flag by name
    pub fn get(&self, name: &str) -> Option<&FeatureFlag> {
        self.flags.get(name)
    }

    /// Check if a flag is enabled for the given context
    pub fn is_enabled(&self, name: &str, context: &FeatureContext) -> bool {
        self.flags
            .get(name)
            .is_some_and(|flag| flag.evaluate(context))
    }

    /// Check if a flag is enabled (without context, for simple boolean flags)
    pub fn is_enabled_simple(&self, name: &str) -> bool {
        self.is_enabled(name, &FeatureContext::default())
    }

    /// Remove a feature flag
    pub fn remove(&mut self, name: &str) -> Option<FeatureFlag> {
        self.flags.remove(name)
    }

    /// List all flags
    pub fn list(&self) -> impl Iterator<Item = &FeatureFlag> {
        self.flags.values()
    }

    /// Merge with another set of flags (other takes precedence)
    pub fn merge(&mut self, other: FeatureFlags) {
        for (name, flag) in other.flags {
            self.flags.insert(name, flag);
        }
    }

    /// Get the number of flags
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boolean_flag() {
        let flag = FeatureFlag::boolean("test.feature", true);
        let ctx = FeatureContext::new();
        assert!(flag.evaluate(&ctx));

        let disabled = FeatureFlag::boolean("test.disabled", false);
        assert!(!disabled.evaluate(&ctx));
    }

    #[test]
    fn test_percentage_flag() {
        // 100% should always be enabled
        let flag = FeatureFlag::percentage("test.percentage", 100);
        let ctx = FeatureContext::with_tenant("tenant-1");
        assert!(flag.evaluate(&ctx));

        // 0% should always be disabled
        let flag_zero = FeatureFlag::percentage("test.zero", 0);
        assert!(!flag_zero.evaluate(&ctx));
    }

    #[test]
    fn test_tenant_based_flag() {
        let flag = FeatureFlag::tenant_based(
            "test.tenant",
            vec!["tenant-a".to_string(), "tenant-b".to_string()],
        );

        let ctx_a = FeatureContext::with_tenant("tenant-a");
        assert!(flag.evaluate(&ctx_a));

        let ctx_c = FeatureContext::with_tenant("tenant-c");
        assert!(!flag.evaluate(&ctx_c));

        let ctx_empty = FeatureContext::new();
        assert!(!flag.evaluate(&ctx_empty));
    }

    #[test]
    fn test_feature_flags_collection() {
        let mut flags = FeatureFlags::new();
        flags.set(FeatureFlag::boolean("feature.a", true));
        flags.set(FeatureFlag::boolean("feature.b", false));

        assert!(flags.is_enabled_simple("feature.a"));
        assert!(!flags.is_enabled_simple("feature.b"));
        assert!(!flags.is_enabled_simple("feature.unknown"));
    }

    #[test]
    fn test_default_flags() {
        let flags = FeatureFlags::with_defaults();
        assert!(flags.get("search.optimization.enabled").is_some());
        assert!(flags.get("terminology.external.enabled").is_some());
    }
}
