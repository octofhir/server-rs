use octofhir_core::{FhirDateTime, ResourceEnvelope, ResourceType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Query filter types for FHIR search patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryFilter {
    /// Exact match filter (e.g., _id=123)
    Exact { field: String, value: String },
    /// Contains filter (e.g., name:contains=John)
    Contains { field: String, value: String },
    /// Date range filter (e.g., date=ge2023-01-01&date=le2023-12-31)
    DateRange {
        field: String,
        start: Option<FhirDateTime>,
        end: Option<FhirDateTime>,
    },
    /// Identifier filter (system|value format)
    Identifier {
        field: String,
        system: Option<String>,
        value: String,
    },
    /// Boolean filter
    Boolean { field: String, value: bool },
    /// Number range filter
    NumberRange {
        field: String,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// Token filter (for coded values)
    Token {
        field: String,
        system: Option<String>,
        code: String,
    },
    /// String prefix filter (starts with)
    Prefix { field: String, value: String },
}

/// Query result with pagination metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Total number of matching resources
    pub total: usize,
    /// Resources in this page
    pub resources: Vec<ResourceEnvelope>,
    /// Offset of the first resource in this page
    pub offset: usize,
    /// Number of resources requested per page
    pub count: usize,
    /// Whether there are more results after this page
    pub has_more: bool,
    /// URL for the next page (if has_more is true)
    pub next_url: Option<String>,
    /// URL for the previous page (if offset > 0)
    pub prev_url: Option<String>,
}

impl QueryResult {
    pub fn new(
        total: usize,
        resources: Vec<ResourceEnvelope>,
        offset: usize,
        count: usize,
    ) -> Self {
        let has_more = offset + resources.len() < total;
        Self {
            total,
            resources,
            offset,
            count,
            has_more,
            next_url: None,
            prev_url: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            total: 0,
            resources: Vec::new(),
            offset: 0,
            count: 0,
            has_more: false,
            next_url: None,
            prev_url: None,
        }
    }

    pub fn with_urls(mut self, next_url: Option<String>, prev_url: Option<String>) -> Self {
        self.next_url = next_url;
        self.prev_url = prev_url;
        self
    }
}

impl QueryFilter {
    /// Check if a resource matches this filter
    pub fn matches(&self, resource: &ResourceEnvelope) -> bool {
        match self {
            QueryFilter::Exact { field, value } => self.match_exact(resource, field, value),
            QueryFilter::Contains { field, value } => self.match_contains(resource, field, value),
            QueryFilter::DateRange { field, start, end } => {
                self.match_date_range(resource, field, start.as_ref(), end.as_ref())
            }
            QueryFilter::Identifier {
                field,
                system,
                value,
            } => self.match_identifier(resource, field, system.as_ref(), value),
            QueryFilter::Boolean { field, value } => self.match_boolean(resource, field, *value),
            QueryFilter::NumberRange { field, min, max } => {
                self.match_number_range(resource, field, *min, *max)
            }
            QueryFilter::Token {
                field,
                system,
                code,
            } => self.match_token(resource, field, system.as_ref(), code),
            QueryFilter::Prefix { field, value } => self.match_prefix(resource, field, value),
        }
    }

    fn match_exact(&self, resource: &ResourceEnvelope, field: &str, value: &str) -> bool {
        // Handle special FHIR fields
        match field {
            "_id" => resource.id == value,
            "_lastUpdated" => {
                if let Ok(target_date) = value.parse::<FhirDateTime>() {
                    resource.meta.last_updated == target_date
                } else {
                    false
                }
            }
            "resourceType" => resource.resource_type.to_string() == value,
            "status" => format!("{:?}", resource.status) == value,
            _ => {
                // Check in resource data
                if let Some(field_value) = resource.get_field(field) {
                    match field_value {
                        Value::String(s) => s == value,
                        Value::Number(n) => n.to_string() == value,
                        Value::Bool(b) => b.to_string() == value,
                        _ => false,
                    }
                } else {
                    false
                }
            }
        }
    }

    fn match_contains(&self, resource: &ResourceEnvelope, field: &str, value: &str) -> bool {
        if let Some(field_value) = resource.get_field(field) {
            self.search_value_recursive(field_value, value, |s, v| {
                s.to_lowercase().contains(&v.to_lowercase())
            })
        } else {
            false
        }
    }

    fn match_date_range(
        &self,
        resource: &ResourceEnvelope,
        field: &str,
        start: Option<&FhirDateTime>,
        end: Option<&FhirDateTime>,
    ) -> bool {
        let date_value = if field == "_lastUpdated" {
            resource.meta.last_updated.clone()
        } else if let Some(Value::String(s)) = resource.get_field(field) {
            match s.parse::<FhirDateTime>() {
                Ok(date) => date,
                Err(_) => return false,
            }
        } else {
            return false;
        };

        if let Some(start) = start
            && &date_value < start
        {
            return false;
        }

        if let Some(end) = end
            && &date_value > end
        {
            return false;
        }

        true
    }

    fn match_identifier(
        &self,
        resource: &ResourceEnvelope,
        field: &str,
        system: Option<&String>,
        value: &str,
    ) -> bool {
        if let Some(field_value) = resource.get_field(field) {
            match field_value {
                Value::Object(obj) => {
                    let system_matches = if let Some(system) = system {
                        obj.get("system").and_then(|v| v.as_str()).is_some_and(|s| s == system)
                    } else {
                        true
                    };

                    let value_matches =
                        obj.get("value").and_then(|v| v.as_str()) == Some(value);

                    system_matches && value_matches
                }
                Value::Array(arr) => arr.iter().any(|item| {
                    if let Value::Object(obj) = item {
                        let system_matches = if let Some(system) = system {
                            obj.get("system").and_then(|v| v.as_str()).is_some_and(|s| s == system)
                        } else {
                            true
                        };

                        let value_matches =
                            obj.get("value").and_then(|v| v.as_str()) == Some(value);

                        system_matches && value_matches
                    } else {
                        false
                    }
                }),
                _ => false,
            }
        } else {
            false
        }
    }

    fn match_boolean(&self, resource: &ResourceEnvelope, field: &str, value: bool) -> bool {
        if let Some(field_value) = resource.get_field(field) {
            match field_value {
                Value::Bool(b) => *b == value,
                Value::String(s) => match s.to_lowercase().as_str() {
                    "true" => value,
                    "false" => !value,
                    _ => false,
                },
                _ => false,
            }
        } else {
            false
        }
    }

    fn match_number_range(
        &self,
        resource: &ResourceEnvelope,
        field: &str,
        min: Option<f64>,
        max: Option<f64>,
    ) -> bool {
        if let Some(field_value) = resource.get_field(field) {
            let number = match field_value {
                Value::Number(n) => n.as_f64().unwrap_or(0.0),
                Value::String(s) => match s.parse::<f64>() {
                    Ok(n) => n,
                    Err(_) => return false,
                },
                _ => return false,
            };

            if let Some(min) = min
                && number < min
            {
                return false;
            }

            if let Some(max) = max
                && number > max
            {
                return false;
            }

            true
        } else {
            false
        }
    }

    fn match_token(
        &self,
        resource: &ResourceEnvelope,
        field: &str,
        system: Option<&String>,
        code: &str,
    ) -> bool {
        // Similar to identifier matching but for coded values
        self.match_identifier(resource, field, system, code)
    }

    fn match_prefix(&self, resource: &ResourceEnvelope, field: &str, value: &str) -> bool {
        if let Some(field_value) = resource.get_field(field) {
            self.search_value_recursive(field_value, value, |s, v| {
                s.to_lowercase().starts_with(&v.to_lowercase())
            })
        } else {
            false
        }
    }

    /// Recursively search through JSON values (arrays and objects) for string matches
    #[allow(clippy::only_used_in_recursion)]
    fn search_value_recursive<F>(&self, value: &Value, search_term: &str, matcher: F) -> bool
    where
        F: Fn(&str, &str) -> bool + Copy,
    {
        match value {
            Value::String(s) => matcher(s, search_term),
            Value::Array(arr) => arr
                .iter()
                .any(|v| self.search_value_recursive(v, search_term, matcher)),
            Value::Object(obj) => obj
                .values()
                .any(|v| self.search_value_recursive(v, search_term, matcher)),
            _ => false,
        }
    }
}

/// Search query with multiple filters and pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub resource_type: ResourceType,
    pub filters: Vec<QueryFilter>,
    pub offset: usize,
    pub count: usize,
    pub sort_field: Option<String>,
    pub sort_ascending: bool,
}

impl SearchQuery {
    pub fn new(resource_type: ResourceType) -> Self {
        Self {
            resource_type,
            filters: Vec::new(),
            offset: 0,
            count: 10, // Default count
            sort_field: None,
            sort_ascending: true,
        }
    }

    pub fn with_filter(mut self, filter: QueryFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_pagination(mut self, offset: usize, count: usize) -> Self {
        self.offset = offset;
        self.count = count;
        self
    }

    pub fn with_sort(mut self, field: String, ascending: bool) -> Self {
        self.sort_field = Some(field);
        self.sort_ascending = ascending;
        self
    }

    /// Check if a resource matches all filters in this query
    pub fn matches(&self, resource: &ResourceEnvelope) -> bool {
        // Check resource type first
        if resource.resource_type != self.resource_type {
            return false;
        }

        // All filters must match
        self.filters.iter().all(|filter| filter.matches(resource))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::ResourceType;
    use serde_json::json;

    fn create_test_patient(id: &str) -> ResourceEnvelope {
        let mut resource = ResourceEnvelope::new(id.to_string(), ResourceType::Patient);
        resource.add_field(
            "name".to_string(),
            json!([{
                "use": "official",
                "family": "Doe",
                "given": ["John", "Q"]
            }]),
        );
        resource.add_field("birthDate".to_string(), json!("1990-01-01"));
        resource.add_field("active".to_string(), json!(true));
        resource.add_field(
            "identifier".to_string(),
            json!([{
                "system": "http://example.com/mrn",
                "value": "12345"
            }]),
        );
        resource
    }

    #[test]
    fn test_query_filter_exact() {
        let resource = create_test_patient("patient-123");

        let filter = QueryFilter::Exact {
            field: "_id".to_string(),
            value: "patient-123".to_string(),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Exact {
            field: "_id".to_string(),
            value: "patient-456".to_string(),
        };
        assert!(!filter.matches(&resource));

        let filter = QueryFilter::Exact {
            field: "birthDate".to_string(),
            value: "1990-01-01".to_string(),
        };
        assert!(filter.matches(&resource));
    }

    #[test]
    fn test_query_filter_contains() {
        let resource = create_test_patient("patient-123");

        let filter = QueryFilter::Contains {
            field: "name".to_string(),
            value: "Doe".to_string(),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Contains {
            field: "name".to_string(),
            value: "Smith".to_string(),
        };
        assert!(!filter.matches(&resource));
    }

    #[test]
    fn test_query_filter_boolean() {
        let resource = create_test_patient("patient-123");

        let filter = QueryFilter::Boolean {
            field: "active".to_string(),
            value: true,
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Boolean {
            field: "active".to_string(),
            value: false,
        };
        assert!(!filter.matches(&resource));
    }

    #[test]
    fn test_query_filter_identifier() {
        let resource = create_test_patient("patient-123");

        let filter = QueryFilter::Identifier {
            field: "identifier".to_string(),
            system: Some("http://example.com/mrn".to_string()),
            value: "12345".to_string(),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Identifier {
            field: "identifier".to_string(),
            system: None,
            value: "12345".to_string(),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Identifier {
            field: "identifier".to_string(),
            system: Some("http://example.com/ssn".to_string()),
            value: "12345".to_string(),
        };
        assert!(!filter.matches(&resource));
    }

    #[test]
    fn test_query_result_new() {
        let resources = vec![create_test_patient("patient-1")];
        let result = QueryResult::new(10, resources.clone(), 0, 5);

        assert_eq!(result.total, 10);
        assert_eq!(result.resources.len(), 1);
        assert_eq!(result.offset, 0);
        assert_eq!(result.count, 5);
        assert!(result.has_more);
    }

    #[test]
    fn test_query_result_empty() {
        let result = QueryResult::empty();

        assert_eq!(result.total, 0);
        assert_eq!(result.resources.len(), 0);
        assert_eq!(result.offset, 0);
        assert_eq!(result.count, 0);
        assert!(!result.has_more);
    }

    #[test]
    fn test_search_query_matches() {
        let resource = create_test_patient("patient-123");

        let query = SearchQuery::new(ResourceType::Patient)
            .with_filter(QueryFilter::Exact {
                field: "_id".to_string(),
                value: "patient-123".to_string(),
            })
            .with_filter(QueryFilter::Boolean {
                field: "active".to_string(),
                value: true,
            });

        assert!(query.matches(&resource));

        // Wrong resource type
        let mut wrong_type = resource.clone();
        wrong_type.resource_type = ResourceType::Organization;
        assert!(!query.matches(&wrong_type));

        // Wrong filter value
        let query_false =
            SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Boolean {
                field: "active".to_string(),
                value: false,
            });

        assert!(!query_false.matches(&resource));
    }

    #[test]
    fn test_search_query_builder() {
        let query = SearchQuery::new(ResourceType::Patient)
            .with_pagination(20, 50)
            .with_sort("name".to_string(), false);

        assert_eq!(query.resource_type, ResourceType::Patient);
        assert_eq!(query.offset, 20);
        assert_eq!(query.count, 50);
        assert_eq!(query.sort_field, Some("name".to_string()));
        assert!(!query.sort_ascending);
    }

    #[test]
    fn test_number_range_filter() {
        let mut resource = create_test_patient("patient-123");
        resource.add_field("age".to_string(), json!(25));

        let filter = QueryFilter::NumberRange {
            field: "age".to_string(),
            min: Some(18.0),
            max: Some(65.0),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::NumberRange {
            field: "age".to_string(),
            min: Some(30.0),
            max: None,
        };
        assert!(!filter.matches(&resource));
    }

    #[test]
    fn test_prefix_filter() {
        let resource = create_test_patient("patient-123");

        let filter = QueryFilter::Prefix {
            field: "name".to_string(),
            value: "Do".to_string(),
        };
        assert!(filter.matches(&resource));

        let filter = QueryFilter::Prefix {
            field: "name".to_string(),
            value: "Smith".to_string(),
        };
        assert!(!filter.matches(&resource));
    }
}
