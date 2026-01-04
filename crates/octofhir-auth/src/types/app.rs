//! App types for authentication.
//!
//! Defines minimal App record structure needed for app authentication.

use serde::{Deserialize, Serialize};

/// Minimal App record for authentication purposes.
///
/// This is a subset of the full App resource, containing only fields
/// needed for Basic Auth extraction and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRecord {
    /// App ID.
    pub id: String,

    /// Human-readable app name.
    pub name: String,

    /// App version (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// App status: active, inactive, suspended.
    /// For authentication, we only care if it's active.
    #[serde(default)]
    pub active: bool,
}

impl AppRecord {
    /// Creates a new AppRecord.
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            version: None,
            active: true,
        }
    }

    /// Returns whether the app is active.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_record_new() {
        let app = AppRecord::new("app-123".to_string(), "Test App".to_string());
        assert_eq!(app.id, "app-123");
        assert_eq!(app.name, "Test App");
        assert!(app.version.is_none());
        assert!(app.is_active());
    }

    #[test]
    fn test_app_record_serialization() {
        let app = AppRecord {
            id: "app-456".to_string(),
            name: "My App".to_string(),
            version: Some("1.0.0".to_string()),
            active: true,
        };

        let json = serde_json::to_value(&app).unwrap();
        assert_eq!(json["id"], "app-456");
        assert_eq!(json["name"], "My App");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["active"], true);
    }
}
