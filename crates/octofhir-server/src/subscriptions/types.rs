//! Core types for FHIR R5 Topic-Based Subscriptions.
//!
//! These types represent parsed and validated subscription configurations
//! for efficient runtime matching and delivery.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

// =============================================================================
// SUBSCRIPTION TOPIC TYPES
// =============================================================================

/// Parsed SubscriptionTopic for efficient in-memory matching.
///
/// This is derived from the FHIR SubscriptionTopic resource but optimized
/// for runtime event matching.
#[derive(Debug, Clone)]
pub struct ParsedSubscriptionTopic {
    /// Resource ID
    pub id: String,

    /// Canonical URL of the topic
    pub url: String,

    /// Human-readable title
    pub title: Option<String>,

    /// Topic status
    pub status: TopicStatus,

    /// Resource triggers that can activate this topic
    pub resource_triggers: Vec<ResourceTrigger>,

    /// Event triggers (non-resource events)
    pub event_triggers: Vec<EventTrigger>,

    /// Filter parameters that subscribers can use
    pub can_filter_by: Vec<FilterDefinition>,

    /// Notification shape configuration
    pub notification_shape: Vec<NotificationShape>,
}

/// Status of a subscription topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TopicStatus {
    Draft,
    Active,
    Retired,
    Unknown,
}

impl Default for TopicStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<&str> for TopicStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "draft" => Self::Draft,
            "active" => Self::Active,
            "retired" => Self::Retired,
            _ => Self::Unknown,
        }
    }
}

/// Resource trigger definition - specifies which resource events trigger notifications.
#[derive(Debug, Clone)]
pub struct ResourceTrigger {
    /// FHIR resource type (e.g., "Patient", "Observation")
    pub resource_type: String,

    /// Supported interactions that trigger this topic
    pub supported_interactions: Vec<TriggerInteraction>,

    /// FHIRPath expression that must evaluate to true for the trigger to fire
    /// Evaluated against the resource
    pub fhirpath_criteria: Option<String>,

    /// Query-style criteria (URL query string format)
    pub query_criteria: Option<QueryCriteria>,

    /// Description of the trigger
    pub description: Option<String>,
}

/// Query-style criteria for resource triggers.
#[derive(Debug, Clone)]
pub struct QueryCriteria {
    /// Query string for previous version (before change)
    pub previous: Option<String>,

    /// Behavior when comparing to previous
    pub result_for_create: QueryResultBehavior,

    /// Query string for current version (after change)
    pub current: Option<String>,

    /// Behavior when comparing current
    pub result_for_delete: QueryResultBehavior,

    /// Whether both previous and current must match
    pub require_both: bool,
}

/// Behavior for query criteria when one side doesn't exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryResultBehavior {
    /// Test passes
    #[default]
    TestPasses,
    /// Test fails
    TestFails,
    /// No test performed
    NoTest,
}

/// Types of resource interactions that can trigger a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerInteraction {
    Create,
    Update,
    Delete,
}

impl From<&str> for TriggerInteraction {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "create" => Self::Create,
            "update" => Self::Update,
            "delete" => Self::Delete,
            _ => Self::Update, // Default fallback
        }
    }
}

impl TriggerInteraction {
    /// Returns the string representation of the interaction.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

/// Event trigger for non-resource events.
#[derive(Debug, Clone)]
pub struct EventTrigger {
    /// Event code/name
    pub event: String,

    /// Description
    pub description: Option<String>,
}

/// Filter definition - describes what filters subscribers can apply.
#[derive(Debug, Clone)]
pub struct FilterDefinition {
    /// Parameter name
    pub filter_parameter: String,

    /// Description of the filter
    pub description: Option<String>,

    /// Resource path for the filter (e.g., "Patient.identifier")
    pub resource: Option<String>,

    /// FHIRPath expression for the filter
    pub filter_definition: Option<String>,

    /// Allowed comparators (eq, ne, gt, lt, etc.)
    pub comparators: Vec<String>,

    /// Allowed modifiers (exact, contains, etc.)
    pub modifiers: Vec<String>,
}

/// Notification shape - describes what data to include in notifications.
#[derive(Debug, Clone)]
pub struct NotificationShape {
    /// Resource type this shape applies to
    pub resource: String,

    /// Elements to include (if empty, include all)
    pub include: Vec<String>,

    /// Related resources to include via _revinclude
    pub rev_include: Vec<String>,
}

// =============================================================================
// SUBSCRIPTION TYPES
// =============================================================================

/// Parsed Subscription for efficient runtime matching.
#[derive(Debug, Clone)]
pub struct ActiveSubscription {
    /// Resource ID
    pub id: String,

    /// Canonical URL of the referenced topic
    pub topic_url: String,

    /// Current status
    pub status: SubscriptionStatus,

    /// Channel configuration
    pub channel: SubscriptionChannel,

    /// Applied filters from subscriber
    pub filter_by: Vec<AppliedFilter>,

    /// Subscription end time (if set)
    pub end_time: Option<OffsetDateTime>,

    /// Heartbeat period in seconds (for WebSocket)
    pub heartbeat_period: Option<u32>,

    /// Maximum events per notification bundle
    pub max_count: Option<u32>,

    /// Contact information for errors
    pub contact: Option<String>,
}

/// Status of a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    /// Initial state, waiting for server to activate
    Requested,
    /// Active and receiving notifications
    Active,
    /// Subscription encountered an error
    Error,
    /// Subscription is turned off
    Off,
    /// Subscription has been entered in error
    EnteredInError,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self::Requested
    }
}

impl From<&str> for SubscriptionStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "requested" => Self::Requested,
            "active" => Self::Active,
            "error" => Self::Error,
            "off" => Self::Off,
            "entered-in-error" => Self::EnteredInError,
            _ => Self::Requested,
        }
    }
}

impl SubscriptionStatus {
    /// Returns the FHIR code for this status.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Active => "active",
            Self::Error => "error",
            Self::Off => "off",
            Self::EnteredInError => "entered-in-error",
        }
    }
}

/// Channel configuration for notification delivery.
#[derive(Debug, Clone)]
pub enum SubscriptionChannel {
    /// REST-hook: HTTP POST to endpoint
    RestHook {
        /// Endpoint URL
        endpoint: String,
        /// Custom headers to include
        headers: Vec<(String, String)>,
        /// Payload content type
        payload_content: PayloadContent,
        /// Content type for the payload
        content_type: String,
    },

    /// WebSocket: Real-time connection
    WebSocket {
        /// Heartbeat period in seconds
        heartbeat_period: u32,
    },

    /// Email: Send notifications via email
    Email {
        /// Email address
        address: String,
    },

    /// Message: Send to messaging endpoint (Kafka, etc.)
    Message {
        /// Endpoint URL
        endpoint: String,
    },
}

impl SubscriptionChannel {
    /// Returns the channel type code.
    pub fn channel_type(&self) -> &'static str {
        match self {
            Self::RestHook { .. } => "rest-hook",
            Self::WebSocket { .. } => "websocket",
            Self::Email { .. } => "email",
            Self::Message { .. } => "message",
        }
    }
}

/// Payload content level for notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadContent {
    /// No payload, just notification that something happened
    Empty,
    /// Include only resource ID
    IdOnly,
    /// Include full resource
    #[default]
    FullResource,
}

impl From<&str> for PayloadContent {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "empty" => Self::Empty,
            "id-only" => Self::IdOnly,
            "full-resource" => Self::FullResource,
            _ => Self::FullResource,
        }
    }
}

/// Filter applied by a subscriber.
#[derive(Debug, Clone)]
pub struct AppliedFilter {
    /// Filter parameter name (from topic's canFilterBy)
    pub filter_parameter: String,

    /// Comparator (eq, ne, gt, etc.)
    pub comparator: Option<String>,

    /// Modifier (exact, contains, etc.)
    pub modifier: Option<String>,

    /// Filter value
    pub value: String,
}

// =============================================================================
// EVENT TYPES
// =============================================================================

/// Event queued for delivery to a subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    /// Unique event ID
    pub id: String,

    /// Subscription ID
    pub subscription_id: String,

    /// Topic URL that triggered this event
    pub topic_url: String,

    /// Event type
    pub event_type: SubscriptionEventType,

    /// Monotonic event number for this subscription
    pub event_number: i64,

    /// Focus resource type (for event-notification)
    pub focus_resource_type: Option<String>,

    /// Focus resource ID (for event-notification)
    pub focus_resource_id: Option<String>,

    /// Trigger event type (for event-notification)
    pub focus_event: Option<TriggerInteraction>,

    /// Pre-rendered notification bundle
    pub notification_bundle: serde_json::Value,

    /// Current status
    pub status: EventStatus,

    /// Creation timestamp
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// Number of delivery attempts
    pub attempts: i32,

    /// Next retry time
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub next_retry_at: Option<OffsetDateTime>,

    /// Last error message
    pub last_error: Option<String>,
}

/// Type of subscription event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionEventType {
    /// Initial handshake when subscription becomes active
    Handshake,
    /// Periodic heartbeat (WebSocket)
    Heartbeat,
    /// Actual event notification
    EventNotification,
}

impl SubscriptionEventType {
    /// Returns the FHIR code for this event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Handshake => "handshake",
            Self::Heartbeat => "heartbeat",
            Self::EventNotification => "event-notification",
        }
    }
}

/// Status of a queued event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventStatus {
    /// Waiting to be processed
    Pending,
    /// Currently being delivered
    Processing,
    /// Successfully delivered
    Delivered,
    /// Permanently failed (max retries exceeded)
    Failed,
    /// TTL exceeded
    Expired,
}

impl Default for EventStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl From<&str> for EventStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pending" => Self::Pending,
            "processing" => Self::Processing,
            "delivered" => Self::Delivered,
            "failed" => Self::Failed,
            "expired" => Self::Expired,
            _ => Self::Pending,
        }
    }
}

// =============================================================================
// DELIVERY TYPES
// =============================================================================

/// Result of a delivery attempt.
#[derive(Debug, Clone)]
pub struct DeliveryResult {
    /// Whether delivery was successful
    pub success: bool,

    /// HTTP status code (for REST-hook)
    pub http_status: Option<u16>,

    /// Response time in milliseconds
    pub response_time_ms: u32,

    /// Error message if failed
    pub error: Option<String>,

    /// Error code if failed
    pub error_code: Option<String>,
}

impl DeliveryResult {
    /// Create a successful result.
    pub fn success(http_status: u16, response_time_ms: u32) -> Self {
        Self {
            success: true,
            http_status: Some(http_status),
            response_time_ms,
            error: None,
            error_code: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>, response_time_ms: u32) -> Self {
        Self {
            success: false,
            http_status: None,
            response_time_ms,
            error: Some(error.into()),
            error_code: None,
        }
    }

    /// Create a failed result with HTTP status.
    pub fn http_failure(http_status: u16, error: impl Into<String>, response_time_ms: u32) -> Self {
        Self {
            success: false,
            http_status: Some(http_status),
            response_time_ms,
            error: Some(error.into()),
            error_code: None,
        }
    }
}

// =============================================================================
// NOTIFICATION BUNDLE
// =============================================================================

/// Builder for creating notification bundles.
pub struct NotificationBundleBuilder {
    subscription_id: String,
    topic_url: String,
    event_type: SubscriptionEventType,
    event_number: i64,
    focus: Option<serde_json::Value>,
}

impl NotificationBundleBuilder {
    /// Create a new notification bundle builder.
    pub fn new(
        subscription_id: String,
        topic_url: String,
        event_type: SubscriptionEventType,
        event_number: i64,
    ) -> Self {
        Self {
            subscription_id,
            topic_url,
            event_type,
            event_number,
            focus: None,
        }
    }

    /// Set the focus resource for event notifications.
    pub fn with_focus(mut self, resource: serde_json::Value) -> Self {
        self.focus = Some(resource);
        self
    }

    /// Build the notification bundle as FHIR Bundle resource.
    pub fn build(self) -> serde_json::Value {
        let timestamp = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut entries = vec![];

        // Add SubscriptionStatus as first entry
        let status = serde_json::json!({
            "resourceType": "SubscriptionStatus",
            "status": "active",
            "type": self.event_type.as_str(),
            "eventsSinceSubscriptionStart": self.event_number.to_string(),
            "notificationEvent": [{
                "eventNumber": self.event_number.to_string(),
                "timestamp": timestamp
            }],
            "subscription": {
                "reference": format!("Subscription/{}", self.subscription_id)
            },
            "topic": self.topic_url
        });

        entries.push(serde_json::json!({
            "fullUrl": format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            "resource": status,
            "request": {
                "method": "GET",
                "url": format!("Subscription/{}/$status", self.subscription_id)
            },
            "response": {
                "status": "200"
            }
        }));

        // Add focus resource if present
        if let Some(focus) = self.focus {
            let resource_type = focus
                .get("resourceType")
                .and_then(|v| v.as_str())
                .unwrap_or("Resource");
            let resource_id = focus
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            entries.push(serde_json::json!({
                "fullUrl": format!("{}/{}", resource_type, resource_id),
                "resource": focus,
                "request": {
                    "method": "GET",
                    "url": format!("{}/{}", resource_type, resource_id)
                },
                "response": {
                    "status": "200"
                }
            }));
        }

        serde_json::json!({
            "resourceType": "Bundle",
            "type": "subscription-notification",
            "timestamp": timestamp,
            "entry": entries
        })
    }
}
