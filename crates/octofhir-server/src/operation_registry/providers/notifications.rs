//! Notifications Operations Provider

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for notification operations
pub struct NotificationsOperationProvider;

impl OperationProvider for NotificationsOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // Resend a failed notification
            OperationDefinition::new(
                "notifications.resend",
                "Resend Notification",
                categories::NOTIFICATIONS,
                vec!["POST".to_string()],
                "/Notification/{id}/$resend",
                modules::NOTIFICATIONS,
            )
            .with_description("Resend a failed notification"),
            // Resend all failed notifications
            OperationDefinition::new(
                "notifications.resend-all",
                "Resend All Failed",
                categories::NOTIFICATIONS,
                vec!["POST".to_string()],
                "/Notification/$resend-all",
                modules::NOTIFICATIONS,
            )
            .with_description("Resend all failed notifications"),
            // Get notification by ID
            OperationDefinition::new(
                "notifications.read",
                "Read Notification",
                categories::NOTIFICATIONS,
                vec!["GET".to_string()],
                "/Notification/{id}",
                modules::NOTIFICATIONS,
            )
            .with_description("Get a notification by ID"),
            // Search notifications
            OperationDefinition::new(
                "notifications.search",
                "Search Notifications",
                categories::NOTIFICATIONS,
                vec!["GET".to_string()],
                "/Notification",
                modules::NOTIFICATIONS,
            )
            .with_description("Search notifications"),
            // Get notification stats
            OperationDefinition::new(
                "notifications.stats",
                "Notification Statistics",
                categories::NOTIFICATIONS,
                vec!["GET".to_string()],
                "/Notification/$stats",
                modules::NOTIFICATIONS,
            )
            .with_description("Get notification statistics (counts by status)"),
            // Cancel a notification
            OperationDefinition::new(
                "notifications.cancel",
                "Cancel Notification",
                categories::NOTIFICATIONS,
                vec!["POST".to_string()],
                "/Notification/{id}/$cancel",
                modules::NOTIFICATIONS,
            )
            .with_description("Cancel a pending notification"),
            // List notification providers
            OperationDefinition::new(
                "notifications.providers.list",
                "List Providers",
                categories::NOTIFICATIONS,
                vec!["GET".to_string()],
                "/NotificationProvider",
                modules::NOTIFICATIONS,
            )
            .with_description("List notification providers"),
            // Get notification provider
            OperationDefinition::new(
                "notifications.providers.read",
                "Read Provider",
                categories::NOTIFICATIONS,
                vec!["GET".to_string()],
                "/NotificationProvider/{id}",
                modules::NOTIFICATIONS,
            )
            .with_description("Get a notification provider by ID"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::NOTIFICATIONS
    }
}
