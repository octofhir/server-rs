//! DICOM JSON and multipart Native DICOM Model XML representations.
//!
//! # Contents
//! Bounded XML serialization of attributes, sequences, person names and bulk URIs.
//! # Invariants
//! Datasets remain transient. XML escapes every source value; binary attributes
//! are never decoded or copied into the database. Both encodings use UTF-8.
use super::negotiation::Preferences;
use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use octofhir_api::ApiError;
use serde_json::Value;

pub(super) const JSON: &str = "application/dicom+json";
pub(super) const XML: &str = "multipart/related; type=\"application/dicom+xml\"";
pub(super) fn escape(value: &str) -> String {
    quick_xml::escape::escape(value).into_owned()
}
pub(super) fn inline_binary(
    data: Vec<u8>,
    preferences: &Preferences,
    max: u64,
) -> Result<Response, ApiError> {
    const MULTIPART: &str =
        "multipart/related; type=\"application/octet-stream\"; transfer-syntax=1.2.840.10008.1.2.1";
    let selected = preferences.select(&[
        MULTIPART,
        "application/octet-stream; transfer-syntax=1.2.840.10008.1.2.1",
    ])?;
    if data.len() as u64 > max {
        return Err(ApiError::bad_request(
            "DICOM bulk data exceeds stream limit",
        ));
    }
    let mut content_range = None;
    let mut status = StatusCode::OK;
    let body = if let Some(range) = preferences.headers.get(header::RANGE) {
        let invalid = || {
            ApiError::issues(
                StatusCode::RANGE_NOT_SATISFIABLE,
                octofhir_api::OperationOutcome::single(
                    "error",
                    "not-supported",
                    "Requested DICOM byte range is unavailable",
                )
                .issue,
            )
        };
        if selected == MULTIPART
            || preferences.headers.get_all(header::RANGE).iter().count() != 1
            || data.is_empty()
        {
            return Err(invalid());
        }
        let value = range
            .to_str()
            .map_err(|_| invalid())?
            .strip_prefix("bytes=")
            .ok_or_else(invalid)?;
        if value.len() > 45 {
            return Err(invalid());
        }
        let (first, last) = value.split_once('-').ok_or_else(invalid)?;
        let (first, last) = if first.is_empty() {
            let suffix = last
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(invalid)?;
            (data.len().saturating_sub(suffix), data.len() - 1)
        } else {
            let first = first.parse::<usize>().map_err(|_| invalid())?;
            let last = if last.is_empty() {
                data.len() - 1
            } else {
                last.parse::<usize>()
                    .map_err(|_| invalid())?
                    .min(data.len() - 1)
            };
            (first, last)
        };
        if first > last || first >= data.len() {
            return Err(invalid());
        }
        content_range = Some(format!("bytes {first}-{last}/{}", data.len()));
        status = StatusCode::PARTIAL_CONTENT;
        data[first..=last].to_vec()
    } else if selected == MULTIPART {
        let boundary = format!("octofhir-{}", uuid::Uuid::new_v4().simple());
        let mut body =
            format!("--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
        body.extend(data);
        body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());
        if body.len() as u64 > max {
            return Err(ApiError::bad_request(
                "DICOM bulk data exceeds stream limit",
            ));
        }
        let mut response = Response::new(Body::from(body));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("{MULTIPART}; boundary={boundary}"))
                .map_err(|_| invalid())?,
        );
        response.headers_mut().insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response.headers_mut().insert(
            header::VARY,
            HeaderValue::from_static("Authorization, Accept, Accept-Charset"),
        );
        return Ok(response);
    } else {
        data
    };
    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Accept, Accept-Charset"),
    );
    if let Some(range) = content_range {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&range).map_err(|_| invalid())?,
        );
    }
    Ok(response)
}
fn invalid() -> ApiError {
    ApiError::internal("PACS metadata cannot be represented as DICOM XML")
}
fn xml_text(value: &str) -> Result<String, ApiError> {
    if value.chars().any(|c| {
        (c.is_control() && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{fffe}' | '\u{ffff}')
    }) {
        return Err(invalid());
    }
    Ok(escape(value))
}
fn checked(out: &str, max: usize) -> Result<(), ApiError> {
    if out.len() > max {
        Err(ApiError::bad_request(
            "DICOM metadata response exceeds configured limit",
        ))
    } else {
        Ok(())
    }
}
fn dataset(value: &Value, out: &mut String, depth: usize, max: usize) -> Result<(), ApiError> {
    if depth > 32 {
        return Err(invalid());
    }
    for (tag, attribute) in value.as_object().ok_or_else(invalid)? {
        if tag.len() != 8 || !tag.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let tag = tag.to_ascii_uppercase();
        let vr = attribute["vr"]
            .as_str()
            .filter(|v| v.len() == 2 && v.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or_else(invalid)?;
        out.push_str(&format!("<DicomAttribute tag=\"{tag}\" vr=\"{vr}\">"));
        if let Some(uri) = attribute.get("BulkDataURI") {
            out.push_str(&format!(
                "<BulkData uri=\"{}\"/>",
                xml_text(uri.as_str().ok_or_else(invalid)?)?
            ));
        } else if let Some(binary) = attribute.get("InlineBinary") {
            out.push_str(&format!(
                "<InlineBinary>{}</InlineBinary>",
                xml_text(binary.as_str().ok_or_else(invalid)?)?
            ));
        } else if let Some(values) = attribute.get("Value") {
            for (index, value) in values.as_array().ok_or_else(invalid)?.iter().enumerate() {
                let number = index + 1;
                if vr == "SQ" {
                    out.push_str(&format!("<Item number=\"{number}\">"));
                    dataset(value, out, depth + 1, max)?;
                    out.push_str("</Item>");
                } else if vr == "PN" {
                    out.push_str(&format!("<PersonName number=\"{number}\">"));
                    if !value.is_null() {
                        let groups = value.as_object().ok_or_else(invalid)?;
                        for group in ["Alphabetic", "Ideographic", "Phonetic"] {
                            if let Some(text) = groups.get(group) {
                                out.push_str(&format!("<{group}>"));
                                if text.as_str().ok_or_else(invalid)?.split('^').count() > 5 {
                                    return Err(invalid());
                                }
                                for (component, text) in [
                                    "FamilyName",
                                    "GivenName",
                                    "MiddleName",
                                    "NamePrefix",
                                    "NameSuffix",
                                ]
                                .iter()
                                .zip(text.as_str().ok_or_else(invalid)?.split('^'))
                                {
                                    if !text.is_empty() {
                                        out.push_str(&format!(
                                            "<{component}>{}</{component}>",
                                            xml_text(text)?
                                        ));
                                    }
                                }
                                out.push_str(&format!("</{group}>"));
                            }
                        }
                    }
                    out.push_str("</PersonName>");
                } else {
                    let text = match value {
                        Value::Null => String::new(),
                        Value::String(v) => v.clone(),
                        Value::Number(v) => v.to_string(),
                        _ => return Err(invalid()),
                    };
                    out.push_str(&format!(
                        "<Value number=\"{number}\">{}</Value>",
                        xml_text(&text)?
                    ));
                }
                checked(out, max)?;
            }
        }
        out.push_str("</DicomAttribute>");
        checked(out, max)?;
    }
    Ok(())
}
pub(super) fn response(
    records: &[Value],
    preferences: &Preferences,
    max: usize,
) -> Result<Response, ApiError> {
    let representation = preferences.select(&[JSON, XML])?;
    let (body, content_type) = if representation == JSON {
        let bytes = serde_json::to_vec(records).map_err(|_| invalid())?;
        if bytes.len() > max {
            return Err(ApiError::bad_request(
                "DICOM metadata response exceeds configured limit",
            ));
        }
        (Body::from(bytes), JSON.into())
    } else {
        let boundary = format!("octofhir-{}", uuid::Uuid::new_v4().simple());
        let mut body = String::new();
        for record in records {
            body.push_str(&format!("--{boundary}\r\nContent-Type: application/dicom+xml; charset=utf-8\r\n\r\n<?xml version=\"1.0\" encoding=\"UTF-8\"?><NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\">") );
            dataset(record, &mut body, 0, max)?;
            body.push_str("</NativeDicomModel>\r\n");
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        checked(&body, max)?;
        (Body::from(body), format!("{XML}; boundary={boundary}"))
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type).map_err(|_| invalid())?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Accept, Accept-Charset"),
    );
    Ok(response)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_model_preserves_sequences_names_and_escaped_bulk_references() {
        let data = json!({"00100010":{"vr":"PN","Value":[{"Alphabetic":"A&B^<Given>"}]},"00400275":{"vr":"SQ","Value":[{"00401001":{"vr":"SH","Value":["request"]}}]},"7FE00010":{"vr":"OW","BulkDataURI":"/bulk?a=1&b=2"}});
        let mut xml = String::new();
        dataset(&data, &mut xml, 0, 65536).unwrap();
        assert!(xml.contains("<FamilyName>A&amp;B</FamilyName>"));
        assert!(xml.contains("<GivenName>&lt;Given&gt;</GivenName>"));
        assert!(xml.contains("<Item number=\"1\">"));
        assert!(xml.contains("uri=\"/bulk?a=1&amp;b=2\""));
        assert!(dataset(&data, &mut String::new(), 0, 8).is_err());
        assert!(
            dataset(
                &json!({"00081030":{"vr":"LO","Value":["\u{0000}"]}}),
                &mut String::new(),
                0,
                1024
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn native_inline_bulk_honors_ranges_and_rejects_compressed_syntax() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "application/octet-stream; transfer-syntax=*"
                .parse()
                .unwrap(),
        );
        headers.insert(header::RANGE, "bytes=1-2".parse().unwrap());
        let mut preferences =
            Preferences::new(&headers, &mut std::collections::BTreeMap::new()).unwrap();
        preferences.validate_category(true).unwrap();
        let response = inline_binary(vec![0, 1, 2, 3], &preferences, 1024).unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 1-2/4");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .as_ref(),
            &[1, 2]
        );
        headers.remove(header::RANGE);
        headers.insert(header::ACCEPT,"multipart/related; type=\"application/octet-stream\"; transfer-syntax=1.2.840.10008.1.2.4.50".parse().unwrap());
        let mut preferences =
            Preferences::new(&headers, &mut std::collections::BTreeMap::new()).unwrap();
        preferences.validate_category(true).unwrap();
        assert_eq!(
            inline_binary(vec![0], &preferences, 1024)
                .unwrap_err()
                .status_code(),
            StatusCode::NOT_ACCEPTABLE
        );
    }
}
