//! Debug test to check why complex types have no elements

use octofhir_fhir_model::provider::FhirVersion;
use octofhir_fhir_model::provider::ModelProvider;
use octofhir_fhirschema::{FhirSchemaModelProvider, get_schemas};
use std::sync::Arc;

#[tokio::test]
async fn debug_complex_type_elements() {
    // Create model provider with R4 schemas
    let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4).clone();
    let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);
    let provider: Arc<dyn ModelProvider + Send + Sync> = Arc::new(provider);

    // Check what complex types are available
    let complex_types = provider.get_complex_types().await.unwrap();
    println!(
        "\n=== Complex types available ({}) ===",
        complex_types.len()
    );
    for (i, ct) in complex_types.iter().take(10).enumerate() {
        println!("{}: {}", i + 1, ct);
    }

    // Check if HumanName and Address are in the list
    let has_human_name = complex_types.contains(&"HumanName".to_string());
    let has_address = complex_types.contains(&"Address".to_string());
    println!("\nHumanName in complex types: {}", has_human_name);
    println!("Address in complex types: {}", has_address);

    // Try to get elements for HumanName
    println!("\n=== Getting elements for HumanName ===");
    match provider.get_elements("HumanName").await {
        Ok(elements) => {
            println!("Found {} elements for HumanName", elements.len());
            for elem in &elements {
                println!("  - {}: {}", elem.name, elem.element_type);
            }
        }
        Err(e) => {
            println!("ERROR getting HumanName elements: {}", e);
        }
    }

    // Try to get elements for Address
    println!("\n=== Getting elements for Address ===");
    match provider.get_elements("Address").await {
        Ok(elements) => {
            println!("Found {} elements for Address", elements.len());
            for elem in &elements {
                println!("  - {}: {}", elem.name, elem.element_type);
            }
        }
        Err(e) => {
            println!("ERROR getting Address elements: {}", e);
        }
    }

    // Try to get TypeInfo for HumanName
    println!("\n=== Getting TypeInfo for HumanName ===");
    match provider.get_type("HumanName").await {
        Ok(Some(type_info)) => {
            println!("TypeInfo found:");
            println!("  name: {:?}", type_info.name);
            println!("  type_name: {:?}", type_info.type_name);
        }
        Ok(None) => {
            println!("No TypeInfo found for HumanName");
        }
        Err(e) => {
            println!("ERROR getting HumanName TypeInfo: {}", e);
        }
    }
}
