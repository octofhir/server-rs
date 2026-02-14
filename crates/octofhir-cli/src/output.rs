use crate::cli::OutputFormat;
use colored::Colorize;
use serde_json::Value;
use tabled::builder::Builder;
use tabled::settings::Style;

pub fn print_value(value: &Value, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
        OutputFormat::Yaml => {
            println!("{}", serde_yaml_to_string(value));
        }
        OutputFormat::Table => {
            print_as_table(value);
        }
    }
}

pub fn print_success(msg: &str) {
    println!("{} {}", "✓".green(), msg);
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", "✗".red(), msg);
}

fn print_as_table(value: &Value) {
    if let Some(entries) = extract_bundle_entries(value) {
        if entries.is_empty() {
            println!("No resources found.");
            return;
        }
        let mut builder = Builder::default();
        builder.push_record(["ID", "ResourceType", "LastUpdated"]);
        for entry in entries {
            let resource = entry.get("resource").unwrap_or(entry);
            let id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let rt = resource
                .get("resourceType")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let updated = resource
                .get("meta")
                .and_then(|m| m.get("lastUpdated"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            builder.push_record([id, rt, updated]);
        }
        let total = value.get("total").and_then(|v| v.as_u64());
        let table = builder.build().with(Style::rounded()).to_string();
        println!("{table}");
        if let Some(total) = total {
            println!("Total: {total}");
        }
    } else {
        // Single resource — show key-value
        let rt = value
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("Resource");
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("-");
        println!("{} {}/{}", "Resource:".cyan(), rt.cyan(), id.cyan());
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}

fn extract_bundle_entries(value: &Value) -> Option<&Vec<Value>> {
    if value.get("resourceType")?.as_str()? == "Bundle" {
        value.get("entry")?.as_array()
    } else {
        None
    }
}

fn serde_yaml_to_string(value: &Value) -> String {
    // Simple YAML-like output without serde_yaml dependency
    format_yaml(value, 0)
}

fn format_yaml(value: &Value, indent: usize) -> String {
    let prefix = " ".repeat(indent);
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.contains('\n') || s.contains(':') || s.contains('#') {
                format!(
                    "|\n{}{}",
                    " ".repeat(indent + 2),
                    s.replace('\n', &format!("\n{}", " ".repeat(indent + 2)))
                )
            } else {
                format!("\"{s}\"")
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                return "[]".to_string();
            }
            let items: Vec<String> = arr
                .iter()
                .map(|v| format!("{prefix}- {}", format_yaml(v, indent + 2)))
                .collect();
            format!("\n{}", items.join("\n"))
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                return "{}".to_string();
            }
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| {
                    let val = format_yaml(v, indent + 2);
                    if val.starts_with('\n') {
                        format!("{prefix}{k}:{val}")
                    } else {
                        format!("{prefix}{k}: {val}")
                    }
                })
                .collect();
            if indent == 0 {
                items.join("\n")
            } else {
                format!("\n{}", items.join("\n"))
            }
        }
    }
}
