//! Debug test to check why name fields show only placeholder

use std::sync::Arc;
use async_graphql::dynamic::Schema;
use async_graphql::{Variables, Value};
use octofhir_fhir_model::provider::FhirVersion;
use octofhir_fhirschema::{FhirSchemaModelProvider, get_schemas};
use octofhir_graphql::{FhirSchemaBuilder, SchemaBuilderConfig};
use octofhir_search::{SearchParameterRegistry, SearchParameter, SearchParameterType};
use serde_json::json;

#[tokio::test]
async fn debug_patient_name_introspection() {
    // Create model provider with R4 schemas
    let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4).clone();
    let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);
    let provider = Arc::new(provider);

    // Create search registry with Patient
    let mut registry = SearchParameterRegistry::new();
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));
    registry.register(SearchParameter::new(
        "given",
        "http://hl7.org/fhir/SearchParameter/Patient-given",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));
    registry.register(SearchParameter::new(
        "gender",
        "http://hl7.org/fhir/SearchParameter/Patient-gender",
        SearchParameterType::Token,
        vec!["Patient".to_string()],
    ));

    // Build schema
    let builder = FhirSchemaBuilder::new(
        Arc::new(registry),
        provider.clone(),
        SchemaBuilderConfig::default(),
    );

    let schema = builder.build().await.expect("Schema build should succeed");

    // Test 1: Introspect Patient type
    println!("\n=== Introspecting Patient type ===");
    let query = r#"
        {
            __type(name: "Patient") {
                name
                fields {
                    name
                    type {
                        name
                        kind
                        ofType {
                            name
                            kind
                        }
                    }
                }
            }
        }
    "#;

    let result = schema.execute(query).await;
    println!("Patient type fields:");
    if let Some(data) = &result.data.into_json().ok() {
        if let Some(type_data) = data.get("__type") {
            if let Some(fields) = type_data.get("fields") {
                if let Some(fields_array) = fields.as_array() {
                    for field in fields_array {
                        if let Some(name) = field.get("name").and_then(|n| n.as_str()) {
                            if name == "name" || name == "address" {
                                println!("  Field: {}", serde_json::to_string_pretty(field).unwrap());
                            }
                        }
                    }
                }
            }
        }
    }

    // Test 2: Introspect HumanName type
    println!("\n=== Introspecting HumanName type ===");
    let query = r#"
        {
            __type(name: "HumanName") {
                name
                kind
                fields {
                    name
                    type {
                        name
                        kind
                    }
                }
            }
        }
    "#;

    let result = schema.execute(query).await;
    println!("HumanName introspection result:");
    if let Some(data) = &result.data.into_json().ok() {
        println!("{}", serde_json::to_string_pretty(data).unwrap());
    }
    if !result.errors.is_empty() {
        println!("Errors: {:?}", result.errors);
    }

    // Test 3: Try to query with name fields expanded
    println!("\n=== Testing actual query with name expansion ===");
    let query = r#"
        {
            __type(name: "Patient") {
                fields {
                    name
                    type {
                        name
                        ofType {
                            name
                            fields {
                                name
                            }
                        }
                    }
                }
            }
        }
    "#;

    let result = schema.execute(query).await;
    if let Some(data) = &result.data.into_json().ok() {
        if let Some(type_data) = data.get("__type") {
            if let Some(fields) = type_data.get("fields") {
                if let Some(fields_array) = fields.as_array() {
                    for field in fields_array {
                        if let Some(name) = field.get("name").and_then(|n| n.as_str()) {
                            if name == "name" {
                                println!("Patient.name field:");
                                println!("{}", serde_json::to_string_pretty(field).unwrap());
                            }
                        }
                    }
                }
            }
        }
    }
}
