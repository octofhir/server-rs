//! Debug test to check GraphQL schema generation for complex types

use octofhir_fhir_model::provider::{FhirVersion, ModelProvider};
use octofhir_fhirschema::{FhirSchemaModelProvider, get_schemas};
use octofhir_graphql::{FhirSchemaBuilder, SchemaBuilderConfig};
use octofhir_search::{SearchParameter, SearchParameterRegistry, SearchParameterType};
use std::sync::Arc;

#[tokio::test]
async fn debug_humanname_in_schema() {
    // Create model provider with R4 schemas
    let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4).clone();
    let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);
    let provider: Arc<dyn ModelProvider + Send + Sync> = Arc::new(provider);

    // Create search registry with Patient (which uses HumanName)
    let mut registry = SearchParameterRegistry::new();
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));

    // Build schema
    let builder = FhirSchemaBuilder::new(
        Arc::new(registry),
        provider.clone(),
        SchemaBuilderConfig::default(),
    );

    println!("\n=== Building GraphQL schema ===");
    let schema = builder.build().await.expect("Schema build should succeed");

    // Get SDL and check for HumanName
    let sdl = schema.sdl();

    println!("\n=== Checking for HumanName in SDL ===");
    if sdl.contains("type HumanName") {
        println!("✅ HumanName type found in schema");

        // Extract and print the HumanName type definition
        if let Some(start) = sdl.find("type HumanName") {
            if let Some(end) = sdl[start..].find("\n\n") {
                let humanname_def = &sdl[start..start + end];
                println!("\n{}", humanname_def);
            }
        }
    } else {
        println!("❌ HumanName type NOT found in schema");
    }

    println!("\n=== Checking for Address in SDL ===");
    if sdl.contains("type Address") {
        println!("✅ Address type found in schema");

        // Extract and print the Address type definition
        if let Some(start) = sdl.find("type Address") {
            if let Some(end) = sdl[start..].find("\n\n") {
                let address_def = &sdl[start..start + end];
                println!("\n{}", address_def);
            }
        }
    } else {
        println!("❌ Address type NOT found in schema");
    }

    // Check for Patient (should definitely be there)
    println!("\n=== Checking for Patient in SDL ===");
    if sdl.contains("type Patient") {
        println!("✅ Patient type found in schema");

        // Extract and print a snippet
        if let Some(start) = sdl.find("type Patient") {
            let snippet_end = std::cmp::min(start + 500, sdl.len());
            let patient_snippet = &sdl[start..snippet_end];
            println!("\n{}...", patient_snippet);
        }
    } else {
        println!("❌ Patient type NOT found in schema");
    }

    // Look for placeholder
    println!("\n=== Checking for _placeholder in SDL ===");
    let placeholder_count = sdl.matches("_placeholder").count();
    println!("Found {} occurrences of '_placeholder'", placeholder_count);

    if placeholder_count > 0 {
        println!("\nTypes with _placeholder:");
        for line in sdl.lines() {
            if line.contains("type ") && sdl[sdl.find(line).unwrap()..].contains("_placeholder") {
                println!("  - {}", line.trim());
            }
        }
    }
}
