use serde::{Deserialize, Serialize};

use crate::types::NotificationChannel;

/// Notification configuration in App subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionNotification {
    /// Reference to NotificationProvider
    pub provider: String, // "NotificationProvider/sendgrid-main"

    /// Notification channel
    pub channel: NotificationChannel,

    /// Template ID
    pub template: String,

    /// How to find recipient from the resource
    pub recipient: RecipientSelector,

    /// Optional delay (for reminders)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<NotificationDelay>,
}

/// How to select notification recipient
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientSelector {
    /// FHIRPath expression to find recipient reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhirpath: Option<String>,

    /// Static recipient (for testing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_email: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_telegram_chat_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_webhook_url: Option<String>,
}

/// Delay configuration for scheduled notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDelay {
    /// Field to calculate delay from (e.g., "start" for Appointment.start)
    pub relative_to: String,

    /// ISO 8601 duration offset (e.g., "-PT24H" for 24h before)
    pub offset: String,
}

/// Event type that triggered the subscription
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionEvent {
    Create,
    Update,
    Delete,
}

/// Context passed to notification handler
#[derive(Debug, Clone)]
pub struct NotificationContext {
    /// The resource that triggered the event
    pub resource: serde_json::Value,

    /// Previous version (for update events)
    pub previous: Option<serde_json::Value>,

    /// Event type
    pub event: SubscriptionEvent,
}

/// Parse ISO 8601 duration (simplified: supports PT{n}H, PT{n}M, PT{n}S, P{n}D)
pub fn parse_iso_duration(s: &str) -> Result<time::Duration, String> {
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-');

    let duration = if s.starts_with("PT") {
        let rest = s.strip_prefix("PT").unwrap();
        if rest.ends_with('H') {
            let hours: i64 = rest
                .trim_end_matches('H')
                .parse()
                .map_err(|e| format!("Invalid hours: {}", e))?;
            time::Duration::hours(hours)
        } else if rest.ends_with('M') {
            let minutes: i64 = rest
                .trim_end_matches('M')
                .parse()
                .map_err(|e| format!("Invalid minutes: {}", e))?;
            time::Duration::minutes(minutes)
        } else if rest.ends_with('S') {
            let seconds: i64 = rest
                .trim_end_matches('S')
                .parse()
                .map_err(|e| format!("Invalid seconds: {}", e))?;
            time::Duration::seconds(seconds)
        } else {
            return Err(format!("Unsupported duration format: {}", s));
        }
    } else if s.starts_with('P') && s.ends_with('D') {
        let days: i64 = s
            .strip_prefix('P')
            .unwrap()
            .trim_end_matches('D')
            .parse()
            .map_err(|e| format!("Invalid days: {}", e))?;
        time::Duration::days(days)
    } else {
        return Err(format!("Unsupported duration format: {}", s));
    };

    Ok(if negative { -duration } else { duration })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso_duration_hours() {
        assert_eq!(
            parse_iso_duration("PT24H").unwrap(),
            time::Duration::hours(24)
        );
        assert_eq!(
            parse_iso_duration("-PT24H").unwrap(),
            time::Duration::hours(-24)
        );
    }

    #[test]
    fn test_parse_iso_duration_minutes() {
        assert_eq!(
            parse_iso_duration("PT30M").unwrap(),
            time::Duration::minutes(30)
        );
        assert_eq!(
            parse_iso_duration("-PT30M").unwrap(),
            time::Duration::minutes(-30)
        );
    }

    #[test]
    fn test_parse_iso_duration_days() {
        assert_eq!(parse_iso_duration("P7D").unwrap(), time::Duration::days(7));
        assert_eq!(
            parse_iso_duration("-P1D").unwrap(),
            time::Duration::days(-1)
        );
    }

    #[test]
    fn test_parse_iso_duration_seconds() {
        assert_eq!(
            parse_iso_duration("PT60S").unwrap(),
            time::Duration::seconds(60)
        );
    }

    #[test]
    fn test_parse_iso_duration_invalid() {
        assert!(parse_iso_duration("invalid").is_err());
        assert!(parse_iso_duration("PT").is_err());
    }
}
