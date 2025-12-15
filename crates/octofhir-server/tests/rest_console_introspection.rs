use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
};
use octofhir_search::{
    parameters::{SearchModifier, SearchParameter, SearchParameterType},
    registry::SearchParameterRegistry,
};
use octofhir_server::rest_console::{self, RestConsoleState};
use octofhir_server::{
    operations::definition::{OperationDefinition, OperationKind},
    operations::registry::OperationRegistry,
};

#[tokio::test]
async fn rest_console_introspection_exposes_metadata() {
    let state = test_state();

    let response = rest_console::introspect(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("etag"));
    assert_eq!(
        response.headers().get("cache-control"),
        Some(&HeaderValue::from_static("public, max-age=60"))
    );

    let payload = serde_json::to_value(rest_console::build_payload(&state)).unwrap();
    assert_eq!(payload["base_path"], "/fhir");
    assert_eq!(payload["fhir_version"], "R4");
    assert!(
        payload["resources"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    );
    assert!(
        payload["operations"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    );
    let first_param = payload["resources"][0]["search_params"][0].clone();
    assert!(
        first_param["modifiers"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    );
    assert!(
        first_param["comparators"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    );
}

fn test_state() -> RestConsoleState {
    let mut registry = SearchParameterRegistry::new();
    let param = SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    )
    .with_expression("Patient.name")
    .with_description("A patient's name")
    .with_modifiers(vec![SearchModifier::Exact])
    .with_comparators(vec!["eq".to_string()]);
    registry.register(param);

    let mut operation_registry = OperationRegistry::new();
    operation_registry.register(OperationDefinition {
        code: "validate".to_string(),
        url: "http://hl7.org/fhir/OperationDefinition/validate".to_string(),
        kind: OperationKind::Operation,
        system: true,
        type_level: true,
        instance: true,
        resource: vec!["Patient".to_string()],
        parameters: vec![],
        affects_state: false,
    });

    RestConsoleState::new(
        Arc::new(registry),
        Arc::new(operation_registry),
        "R4".to_string(),
    )
}
