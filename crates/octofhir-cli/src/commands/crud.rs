use std::fs;
use std::io::{self, Read};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::cli::OutputFormat;
use crate::client::FhirClient;
use crate::output::{print_success, print_value};

fn parse_reference(reference: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = reference.splitn(2, '/').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid reference \"{reference}\". Expected format: ResourceType/id");
    }
    Ok((parts[0], parts[1]))
}

fn read_body(file: &Option<String>) -> Result<serde_json::Value> {
    let content = match file {
        Some(path) => {
            fs::read_to_string(path).with_context(|| format!("Failed to read file: {path}"))?
        }
        None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read from stdin")?;
            buf
        }
    };
    serde_json::from_str(&content).context("Invalid JSON")
}

pub async fn get(client: &FhirClient, reference: &str, format: OutputFormat) -> Result<()> {
    let (rt, id) = parse_reference(reference)?;
    let resource = client.read(rt, id).await?;
    print_value(&resource, format);
    Ok(())
}

pub async fn create(
    client: &FhirClient,
    resource_type: &str,
    file: &Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let body = read_body(file)?;
    let created = client.create(resource_type, &body).await?;
    let id = created.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    print_success(&format!("Created {}/{}", resource_type.cyan(), id.cyan()));
    print_value(&created, format);
    Ok(())
}

pub async fn update(
    client: &FhirClient,
    reference: &str,
    file: &Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let (rt, id) = parse_reference(reference)?;
    let body = read_body(file)?;
    let updated = client.update(rt, id, &body).await?;
    print_success(&format!("Updated {}/{}", rt.cyan(), id.cyan()));
    print_value(&updated, format);
    Ok(())
}

pub async fn delete(client: &FhirClient, reference: &str) -> Result<()> {
    let (rt, id) = parse_reference(reference)?;
    client.delete(rt, id).await?;
    print_success(&format!("Deleted {}/{}", rt.cyan(), id.cyan()));
    Ok(())
}

pub async fn history(client: &FhirClient, reference: &str, format: OutputFormat) -> Result<()> {
    let (rt, id) = parse_reference(reference)?;
    let bundle = client.history(rt, id).await?;
    print_value(&bundle, format);
    Ok(())
}
