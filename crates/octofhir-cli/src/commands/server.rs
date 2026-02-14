use anyhow::Result;
use colored::Colorize;

use crate::cli::OutputFormat;
use crate::client::FhirClient;
use crate::output::print_value;

pub async fn metadata(client: &FhirClient, format: OutputFormat) -> Result<()> {
    let cs = client.metadata().await?;

    if matches!(format, OutputFormat::Table) {
        let fhir_version = cs
            .get("fhirVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let software_name = cs
            .get("software")
            .and_then(|s| s.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let software_version = cs
            .get("software")
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let status = cs
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        println!("{}: {} {}", "Server".cyan(), software_name, software_version);
        println!("{}: {}", "FHIR Version".cyan(), fhir_version);
        println!("{}: {}", "Status".cyan(), status);

        if let Some(rest) = cs.get("rest").and_then(|v| v.as_array()) {
            for r in rest {
                if let Some(resources) = r.get("resource").and_then(|v| v.as_array()) {
                    println!(
                        "{}: {} resource types",
                        "Resources".cyan(),
                        resources.len()
                    );
                    let types: Vec<&str> = resources
                        .iter()
                        .filter_map(|r| r.get("type").and_then(|v| v.as_str()))
                        .collect();
                    if !types.is_empty() {
                        println!("  {}", types.join(", "));
                    }
                }
            }
        }
    } else {
        print_value(&cs, format);
    }
    Ok(())
}

pub async fn status(client: &FhirClient, server: &str) -> Result<()> {
    let (code, body) = client.health().await?;
    if code == 200 {
        println!("{} {} is {}", "✓".green(), server.cyan(), "healthy".green());
        if !body.is_empty() {
            println!("  {body}");
        }
    } else {
        println!(
            "{} {} returned {} {}",
            "✗".red(),
            server.cyan(),
            code.to_string().red(),
            body
        );
    }
    Ok(())
}
