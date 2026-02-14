use anyhow::Result;

use crate::cli::OutputFormat;
use crate::client::FhirClient;
use crate::output::print_value;

pub async fn search(
    client: &FhirClient,
    resource_type: &str,
    raw_params: &[String],
    count: Option<u32>,
    format: OutputFormat,
) -> Result<()> {
    let mut params: Vec<(String, String)> = raw_params
        .iter()
        .map(|p| {
            let mut parts = p.splitn(2, '=');
            let key = parts.next().unwrap_or("").to_string();
            let value = parts.next().unwrap_or("").to_string();
            (key, value)
        })
        .collect();

    if let Some(c) = count {
        params.push(("_count".to_string(), c.to_string()));
    }

    let bundle = client.search(resource_type, &params).await?;
    print_value(&bundle, format);
    Ok(())
}
