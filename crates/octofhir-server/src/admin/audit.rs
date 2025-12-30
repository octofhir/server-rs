//! Admin audit analytics endpoints.
//!
//! Provides `/admin/audit/$analytics` endpoint for aggregating audit event data.

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::server::AppState;
use octofhir_auth::middleware::AdminAuth;

/// Query parameters for audit analytics
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyticsQuery {
    /// Start of time range (ISO 8601 datetime)
    #[serde(rename = "_since")]
    pub since: Option<String>,
    /// End of time range (ISO 8601 datetime)
    #[serde(rename = "_until")]
    pub until: Option<String>,
}

/// Response from audit analytics endpoint
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsResponse {
    /// Total count of audit events
    pub total: usize,
    /// Breakdown by action type
    pub by_action: Vec<ActionCount>,
    /// Breakdown by outcome
    pub by_outcome: Vec<OutcomeCount>,
    /// Breakdown by actor type
    pub by_actor_type: Vec<ActorTypeCount>,
    /// Activity over time (hourly buckets)
    pub activity_timeline: Vec<TimelinePoint>,
    /// Top resources by access count
    pub top_resources: Vec<ResourceCount>,
    /// Failed action attempts
    pub failed_attempts: usize,
}

/// Count by action type
#[derive(Debug, Clone, Serialize)]
pub struct ActionCount {
    pub action: String,
    pub count: usize,
}

/// Count by outcome
#[derive(Debug, Clone, Serialize)]
pub struct OutcomeCount {
    pub outcome: String,
    pub count: usize,
}

/// Count by actor type
#[derive(Debug, Clone, Serialize)]
pub struct ActorTypeCount {
    pub actor_type: String,
    pub count: usize,
}

/// Timeline data point
#[derive(Debug, Clone, Serialize)]
pub struct TimelinePoint {
    pub timestamp: String,
    pub count: usize,
}

/// Count by resource
#[derive(Debug, Clone, Serialize)]
pub struct ResourceCount {
    pub resource_type: String,
    pub count: usize,
}

/// Get audit analytics aggregations.
///
/// Returns aggregated statistics about audit events, including:
/// - Total event count
/// - Breakdown by action type
/// - Breakdown by outcome
/// - Activity timeline
/// - Top accessed resources
/// - Failed attempt count
///
/// # Authorization
///
/// Requires admin authentication.
pub async fn get_audit_analytics(
    _auth: AdminAuth,
    State(state): State<AppState>,
    Query(params): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    // Build search parameters
    let mut query_parts = Vec::new();

    // Add date range filters
    if let Some(ref since) = params.since {
        query_parts.push(format!("date=ge{}", since));
    }
    if let Some(ref until) = params.until {
        query_parts.push(format!("date=le{}", until));
    }

    // Sort by date descending, limit to 1000 for analysis
    query_parts.push("_count=1000".to_string());
    query_parts.push("_sort=-date".to_string());

    let query_string = query_parts.join("&");
    let search_params = octofhir_search::parse_query_string(&query_string, 1000, 1000);

    // Search for AuditEvent resources
    match state.storage.search("AuditEvent", &search_params).await {
        Ok(result) => {
            let events = &result.entries;
            let total = events.len();

            // Aggregate by action (subtype)
            let mut action_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for event in events {
                if let Some(subtypes) = event.resource.get("subtype").and_then(|s| s.as_array()) {
                    for subtype in subtypes {
                        if let Some(code) = subtype.get("code").and_then(|c| c.as_str()) {
                            *action_counts.entry(code.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            let by_action: Vec<ActionCount> = action_counts
                .into_iter()
                .map(|(action, count)| ActionCount { action, count })
                .collect();

            // Aggregate by outcome
            let mut outcome_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut failed_attempts = 0;
            for event in events {
                if let Some(outcome) = event.resource.get("outcome").and_then(|o| o.as_str()) {
                    let outcome_name = match outcome {
                        "0" => "success",
                        "4" => "minor_failure",
                        "8" => "serious_failure",
                        "12" => "major_failure",
                        _ => outcome,
                    };
                    *outcome_counts
                        .entry(outcome_name.to_string())
                        .or_insert(0) += 1;
                    if outcome != "0" {
                        failed_attempts += 1;
                    }
                }
            }
            let by_outcome: Vec<OutcomeCount> = outcome_counts
                .into_iter()
                .map(|(outcome, count)| OutcomeCount { outcome, count })
                .collect();

            // Aggregate by actor type
            let mut actor_type_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for event in events {
                if let Some(agents) = event.resource.get("agent").and_then(|a| a.as_array()) {
                    for agent in agents {
                        if let Some(types) = agent.get("type").and_then(|t| t.as_array()) {
                            for t in types {
                                if let Some(coding) = t.get("coding").and_then(|c| c.as_array()) {
                                    for c in coding {
                                        if let Some(code) = c.get("code").and_then(|c| c.as_str())
                                        {
                                            *actor_type_counts
                                                .entry(code.to_string())
                                                .or_insert(0) += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let by_actor_type: Vec<ActorTypeCount> = actor_type_counts
                .into_iter()
                .map(|(actor_type, count)| ActorTypeCount { actor_type, count })
                .collect();

            // Activity timeline (aggregate by hour)
            let mut timeline: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for event in events {
                if let Some(recorded) = event.resource.get("recorded").and_then(|r| r.as_str()) {
                    // Extract hour from ISO timestamp
                    let hour = if recorded.len() >= 13 {
                        format!("{}:00:00Z", &recorded[..13])
                    } else {
                        recorded.to_string()
                    };
                    *timeline.entry(hour).or_insert(0) += 1;
                }
            }
            let mut activity_timeline: Vec<TimelinePoint> = timeline
                .into_iter()
                .map(|(timestamp, count)| TimelinePoint { timestamp, count })
                .collect();
            activity_timeline.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            // Top resources
            let mut resource_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for event in events {
                if let Some(entities) = event.resource.get("entity").and_then(|e| e.as_array()) {
                    for entity in entities {
                        if let Some(what) = entity.get("what") {
                            if let Some(types) = what.get("type").and_then(|t| t.as_str()) {
                                *resource_counts.entry(types.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            let mut top_resources: Vec<ResourceCount> = resource_counts
                .into_iter()
                .map(|(resource_type, count)| ResourceCount {
                    resource_type,
                    count,
                })
                .collect();
            top_resources.sort_by(|a, b| b.count.cmp(&a.count));
            top_resources.truncate(10);

            let response = AnalyticsResponse {
                total,
                by_action,
                by_outcome,
                by_actor_type,
                activity_timeline,
                top_resources,
                failed_attempts,
            };

            (StatusCode::OK, Json(json!(response))).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to fetch audit events for analytics");
            let body = json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "error",
                    "code": "exception",
                    "diagnostics": format!("Failed to fetch audit analytics: {}", e)
                }]
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}
