//! Instance-bound metadata rewriting without arbitrary BulkDataURI proxying.
//!
//! # Invariants
//! A bulk target is re-derived from freshly retrieved authorized instance metadata.
//! It must remain under that exact configured instance, on the same origin. Tokens
//! are DICOM tag/sequence paths, never encoded URLs. Nested metadata is bounded.
use octofhir_api::ApiError;
use serde_json::Value;
use url::Url;
fn invalid() -> ApiError {
    ApiError::bad_request("PACS BulkDataURI is outside the authorized instance")
}
pub(super) fn resolve(base: &Url, uri: &str) -> Result<Url, ApiError> {
    if uri.contains(['%', '\\']) {
        return Err(invalid());
    }
    let target = base.join(uri).map_err(|_| invalid())?;
    let prefix = base.path().strip_suffix("metadata").ok_or_else(invalid)?;
    if target.origin() != base.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
        || !(target.path() == prefix.trim_end_matches('/')
            || (target.path().starts_with(prefix) && target.path().len() > prefix.len()))
    {
        return Err(invalid());
    }
    // BulkDataURI has no prescribed suffix in DICOMweb. The exact instance
    // prefix and origin are the security boundary; archive-specific bulk paths
    // are allowed only when obtained from this instance's fresh metadata.
    if target.path().strip_prefix(prefix) == Some("metadata") {
        return Err(invalid());
    }
    Ok(target)
}
pub(super) fn rewrite(dataset: &mut Value, base: &Url, gateway: &str) -> Result<(), ApiError> {
    walk(dataset, base, gateway, "", 0)
}
fn walk(
    dataset: &mut Value,
    base: &Url,
    gateway: &str,
    prefix: &str,
    depth: usize,
) -> Result<(), ApiError> {
    if depth > 32 {
        return Err(invalid());
    }
    let object = dataset.as_object_mut().ok_or_else(invalid)?;
    for (tag, attribute) in object {
        if tag.len() != 8 || !tag.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let token = if prefix.is_empty() {
            tag.clone()
        } else {
            format!("{prefix}.{tag}")
        };
        if let Some(uri) = attribute.get("BulkDataURI") {
            resolve(base, uri.as_str().ok_or_else(invalid)?)?;
            attribute["BulkDataURI"] = Value::String(format!("{gateway}/bulkdata/{token}"));
        }
        if attribute["vr"] == "SQ"
            && let Some(items) = attribute.get_mut("Value").and_then(Value::as_array_mut)
        {
            for (index, item) in items.iter_mut().enumerate() {
                walk(item, base, gateway, &format!("{token}.{index}"), depth + 1)?;
            }
        }
    }
    Ok(())
}
pub(super) fn lookup(dataset: &Value, token: &str, base: &Url) -> Result<Url, ApiError> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() > 65 || parts.len() % 2 != 1 {
        return Err(invalid());
    }
    let mut dataset = dataset;
    for (index, tag) in parts.iter().enumerate().step_by(2) {
        if tag.len() != 8 || !tag.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let attribute = dataset.get(*tag).ok_or_else(invalid)?;
        if index == parts.len() - 1 {
            return resolve(base, attribute["BulkDataURI"].as_str().ok_or_else(invalid)?);
        }
        if attribute["vr"] != "SQ" {
            return Err(invalid());
        }
        let item = parts[index + 1].parse::<usize>().map_err(|_| invalid())?;
        dataset = attribute["Value"].get(item).ok_or_else(invalid)?;
    }
    Err(invalid())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bulk_uri_cannot_escape_instance_or_origin() {
        let base =
            Url::parse("https://pacs/dicom/studies/1/series/2/instances/3/metadata").unwrap();
        for uri in [
            "https://evil/pixeldata",
            "../4/pixeldata",
            "../../pixeldata",
            "pixeldata?token=x",
            "bulkdata/%2e%2e/metadata",
            "metadata",
            "//user:pass@pacs/dicom/studies/1/series/2/instances/3/pixeldata",
        ] {
            assert!(resolve(&base, uri).is_err(), "{uri}");
        }
        assert!(resolve(&base, "./bulkdata/7FE00010").is_ok());
    }
}
