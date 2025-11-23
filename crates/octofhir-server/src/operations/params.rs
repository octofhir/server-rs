//! Operation parameter extraction and conversion.
//!
//! This module provides utilities for extracting operation parameters from
//! HTTP requests (both GET and POST) and converting them to FHIR Parameters
//! resources.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Query, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

/// Operation parameters extracted from an HTTP request.
///
/// Parameters can come from either:
/// - GET request query string (converted to Parameters resource)
/// - POST request body (either Parameters resource or single resource)
#[derive(Debug, Clone)]
pub enum OperationParams {
    /// Parameters from query string (GET request)
    Get(HashMap<String, String>),
    /// Parameters from request body (POST request)
    Post(Value),
}

impl OperationParams {
    /// Converts the operation parameters to a FHIR Parameters resource.
    ///
    /// For GET requests, query parameters are converted to Parameters.parameter
    /// entries with valueString.
    ///
    /// For POST requests:
    /// - If the body is already a Parameters resource, it's returned as-is
    /// - Otherwise, the body is wrapped as a "resource" parameter
    pub fn to_value(&self) -> Value {
        match self {
            Self::Get(params) => {
                let parameters: Vec<Value> = params
                    .iter()
                    .map(|(name, value)| {
                        json!({
                            "name": name,
                            "valueString": value
                        })
                    })
                    .collect();

                json!({
                    "resourceType": "Parameters",
                    "parameter": parameters
                })
            }
            Self::Post(value) => {
                if value.get("resourceType").and_then(|v| v.as_str()) == Some("Parameters") {
                    value.clone()
                } else {
                    // Single resource parameter - wrap it
                    json!({
                        "resourceType": "Parameters",
                        "parameter": [{
                            "name": "resource",
                            "resource": value
                        }]
                    })
                }
            }
        }
    }

    /// Gets a named parameter value from the Parameters resource.
    ///
    /// For valueX types, returns the value directly.
    /// For resource parameters, returns the resource.
    pub fn get_parameter(&self, name: &str) -> Option<Value> {
        let params = self.to_value();

        params
            .get("parameter")
            .and_then(|arr| arr.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
            })
            .and_then(|p| {
                p.as_object().and_then(|obj| {
                    // Find the value field (valueString, valueCode, resource, etc.)
                    obj.iter()
                        .find(|(k, _)| k.starts_with("value") || *k == "resource")
                        .map(|(_, v)| v.clone())
                })
            })
    }

    /// Gets all parameter values with the given name.
    ///
    /// Useful for repeating parameters.
    pub fn get_parameters(&self, name: &str) -> Vec<Value> {
        let params = self.to_value();

        params
            .get("parameter")
            .and_then(|arr| arr.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
                    .filter_map(|p| {
                        p.as_object().and_then(|obj| {
                            obj.iter()
                                .find(|(k, _)| k.starts_with("value") || *k == "resource")
                                .map(|(_, v)| v.clone())
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Gets a string parameter value.
    pub fn get_string(&self, name: &str) -> Option<String> {
        self.get_parameter(name)
            .and_then(|v| v.as_str().map(String::from))
    }

    /// Gets a boolean parameter value.
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_parameter(name).and_then(|v| v.as_bool())
    }

    /// Gets a resource parameter value.
    pub fn get_resource(&self, name: &str) -> Option<Value> {
        self.get_parameter(name).filter(|v| v.is_object())
    }

    /// Checks if the parameters are empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Get(params) => params.is_empty(),
            Self::Post(value) => value
                .get("parameter")
                .and_then(|arr| arr.as_array())
                .map(|arr| arr.is_empty())
                .unwrap_or(true),
        }
    }
}

/// Rejection type for operation parameter extraction failures.
#[derive(Debug)]
pub struct OperationParamsRejection {
    message: String,
}

impl IntoResponse for OperationParamsRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "error",
                    "code": "invalid",
                    "diagnostics": self.message
                }]
            })),
        )
            .into_response()
    }
}

impl<S> FromRequest<S> for OperationParams
where
    S: Send + Sync,
{
    type Rejection = OperationParamsRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let method = req.method().clone();

        if method == axum::http::Method::GET {
            // Extract query parameters from parts
            let (mut parts, body) = req.into_parts();
            let Query(params): Query<HashMap<String, String>> =
                Query::from_request_parts(&mut parts, state)
                    .await
                    .map_err(|e| OperationParamsRejection {
                        message: format!("Failed to parse query parameters: {}", e),
                    })?;
            // Reconstruct request for potential further use (though we don't need it here)
            drop(body);
            Ok(OperationParams::Get(params))
        } else {
            // Extract JSON body for POST
            let Json(value): Json<Value> =
                Json::from_request(req, state)
                    .await
                    .map_err(|e| OperationParamsRejection {
                        message: format!("Failed to parse request body: {}", e),
                    })?;
            Ok(OperationParams::Post(value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_params_to_value() {
        let mut params = HashMap::new();
        params.insert("code".to_string(), "123".to_string());
        params.insert("system".to_string(), "http://example.com".to_string());

        let op_params = OperationParams::Get(params);
        let value = op_params.to_value();

        assert_eq!(value["resourceType"], "Parameters");
        let params_array = value["parameter"].as_array().unwrap();
        assert_eq!(params_array.len(), 2);
    }

    #[test]
    fn test_post_parameters_resource() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "code",
                "valueCode": "test"
            }]
        });

        let op_params = OperationParams::Post(params.clone());
        let value = op_params.to_value();

        assert_eq!(value, params);
    }

    #[test]
    fn test_post_single_resource() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "123"
        });

        let op_params = OperationParams::Post(resource);
        let value = op_params.to_value();

        assert_eq!(value["resourceType"], "Parameters");
        assert_eq!(value["parameter"][0]["name"], "resource");
        assert_eq!(value["parameter"][0]["resource"]["resourceType"], "Patient");
    }

    #[test]
    fn test_get_parameter() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "url",
                "valueUri": "http://example.com/fhir"
            }]
        });

        let op_params = OperationParams::Post(params);
        let url = op_params.get_parameter("url");

        assert!(url.is_some());
        assert_eq!(url.unwrap(), "http://example.com/fhir");
    }

    #[test]
    fn test_get_string() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "test".to_string());

        let op_params = OperationParams::Get(params);
        let name = op_params.get_string("name");

        assert_eq!(name, Some("test".to_string()));
    }

    #[test]
    fn test_get_resource() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [{
                "name": "resource",
                "resource": {
                    "resourceType": "Patient",
                    "id": "123"
                }
            }]
        });

        let op_params = OperationParams::Post(params);
        let resource = op_params.get_resource("resource");

        assert!(resource.is_some());
        assert_eq!(resource.unwrap()["resourceType"], "Patient");
    }

    #[test]
    fn test_get_parameters_multiple() {
        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "code", "valueCode": "a"},
                {"name": "code", "valueCode": "b"},
                {"name": "other", "valueString": "x"}
            ]
        });

        let op_params = OperationParams::Post(params);
        let codes = op_params.get_parameters("code");

        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0], "a");
        assert_eq!(codes[1], "b");
    }

    #[test]
    fn test_is_empty() {
        let empty_get = OperationParams::Get(HashMap::new());
        assert!(empty_get.is_empty());

        let empty_post = OperationParams::Post(json!({
            "resourceType": "Parameters",
            "parameter": []
        }));
        assert!(empty_post.is_empty());

        let non_empty_get = OperationParams::Get({
            let mut m = HashMap::new();
            m.insert("a".to_string(), "b".to_string());
            m
        });
        assert!(!non_empty_get.is_empty());
    }
}
