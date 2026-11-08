//! Instance-bound metadata rewriting without arbitrary BulkDataURI proxying.
//!
//! # Contents
//! Opaque source URI admission, rewritten attribute tokens and fresh lookup.
//!
//! # Invariants
//! A bulk target is re-derived from freshly retrieved authorized instance metadata.
//! The PACS supplies the opaque attribute URI, which must stay under the configured
//! DICOMweb service on the same origin. Tokens are DICOM tag/sequence paths, never
//! caller-supplied URLs. Nested metadata and URI sizes are bounded.
use base64::Engine;
use octofhir_api::ApiError;
use serde_json::Value;
use url::Url;
pub(super) enum Payload {
    Uri(Url),
    Inline(Vec<u8>),
}
fn invalid() -> ApiError {
    ApiError::bad_request("PACS BulkDataURI is outside the configured DICOMweb service")
}
pub(super) fn resolve(base: &Url, service: &Url, uri: &str) -> Result<Url, ApiError> {
    if uri.len() > 8192 || uri.contains('\\') || uri.chars().any(char::is_control) {
        return Err(invalid());
    }
    let target = base.join(uri).map_err(|_| invalid())?;
    let decoded = urlencoding::decode(target.path()).map_err(|_| invalid())?;
    let root = urlencoding::decode(service.path()).map_err(|_| invalid())?;
    let prefix = format!("{}/", root.trim_end_matches('/'));
    if target.origin() != service.origin()
        || base.origin() != service.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.fragment().is_some()
        || !decoded.starts_with(&prefix)
        || decoded == prefix
        || decoded.contains(['%', '\\'])
        || decoded.chars().any(char::is_control)
        || decoded.split('/').any(|part| matches!(part, "." | ".."))
        || target.path() == base.path()
    {
        return Err(invalid());
    }
    // No request can name this target. A fresh read of the authorized SOP's
    // attribute is the binding; opaque PACS URIs need not encode an instance UID.
    Ok(target)
}
pub(super) fn rewrite(
    dataset: &mut Value,
    base: &Url,
    service: &Url,
    gateway: &str,
) -> Result<(), ApiError> {
    walk(dataset, base, service, gateway, "", 0)
}
fn walk(
    dataset: &mut Value,
    base: &Url,
    service: &Url,
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
            resolve(base, service, uri.as_str().ok_or_else(invalid)?)?;
            attribute["BulkDataURI"] = Value::String(format!("{gateway}/bulkdata/{token}"));
        }
        if attribute["vr"] == "SQ"
            && let Some(items) = attribute.get_mut("Value").and_then(Value::as_array_mut)
        {
            for (index, item) in items.iter_mut().enumerate() {
                walk(
                    item,
                    base,
                    service,
                    gateway,
                    &format!("{token}.{index}"),
                    depth + 1,
                )?;
            }
        }
    }
    Ok(())
}
fn attribute<'a>(dataset: &'a Value, token: &str) -> Result<&'a Value, ApiError> {
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
            return Ok(attribute);
        }
        if attribute["vr"] != "SQ" {
            return Err(invalid());
        }
        let item = parts[index + 1].parse::<usize>().map_err(|_| invalid())?;
        dataset = attribute["Value"].get(item).ok_or_else(invalid)?;
    }
    Err(invalid())
}
pub(super) fn payload(
    dataset: &Value,
    token: &str,
    base: &Url,
    service: &Url,
) -> Result<Payload, ApiError> {
    let attribute = attribute(dataset, token)?;
    if let Some(uri) = attribute.get("BulkDataURI").and_then(Value::as_str) {
        Ok(Payload::Uri(resolve(base, service, uri)?))
    } else if let Some(value) = attribute.get("InlineBinary").and_then(Value::as_str) {
        Ok(Payload::Inline(
            base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|_| invalid())?,
        ))
    } else {
        Err(invalid())
    }
}
pub(super) fn tokens(dataset: &Value) -> Result<Vec<String>, ApiError> {
    fn collect(
        dataset: &Value,
        prefix: &str,
        depth: usize,
        output: &mut Vec<String>,
    ) -> Result<(), ApiError> {
        if depth > 32 {
            return Err(invalid());
        }
        for (tag, attribute) in dataset.as_object().ok_or_else(invalid)? {
            if tag.len() != 8 || !tag.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(invalid());
            }
            let token = if prefix.is_empty() {
                tag.clone()
            } else {
                format!("{prefix}.{tag}")
            };
            if attribute.get("BulkDataURI").is_some() || attribute.get("InlineBinary").is_some() {
                output.push(token.clone());
            }
            if attribute["vr"] == "SQ"
                && let Some(items) = attribute["Value"].as_array()
            {
                for (index, item) in items.iter().enumerate() {
                    collect(item, &format!("{token}.{index}"), depth + 1, output)?;
                }
            }
        }
        Ok(())
    }
    let mut output = vec![];
    collect(dataset, "", 0, &mut output)?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_bulk_uri_cannot_escape_service_or_origin() {
        let base =
            Url::parse("https://pacs/dicom/studies/1/series/2/instances/3/metadata").unwrap();
        let service = Url::parse("https://pacs/dicom").unwrap();
        for uri in [
            "https://evil/pixeldata",
            "/admin/secrets",
            "/dicom-other/bulk/1",
            "/dicom/%2f..%2fadmin",
            "/dicom/%252e%252e/admin",
            "bulkdata/%2e%2e/metadata",
            "metadata",
            "//user:pass@pacs/dicom/studies/1/series/2/instances/3/pixeldata",
        ] {
            assert!(resolve(&base, &service, uri).is_err(), "{uri}");
        }
        assert!(resolve(&base, &service, "./bulkdata/7FE00010").is_ok());
        assert!(resolve(&base, &service, "/dicom/bulk/opaque%20value?version=1").is_ok());
    }
}
