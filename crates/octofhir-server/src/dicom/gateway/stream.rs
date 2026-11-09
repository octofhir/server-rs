//! Backpressured PACS bodies with bounded lifetime, size and idle time.
//!
//! # Contents
//! Guarded retrieval, sanitized status/header handling and bounded body streaming.
//!
//! # Invariants
//! Only selected negotiation/range headers cross the boundary. No buffering task
//! or unbounded channel exists; dropping the downstream body drops the PACS request
//! and concurrency permit. Remote errors/bodies/cookies are never relayed as HTML.
use super::DicomGatewayConfig;
use crate::dicom::client::DicomWebClient;
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use octofhir_api::ApiError;
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;

pub(super) async fn retrieve(
    client: &DicomWebClient,
    url: url::Url,
    incoming: &HeaderMap,
    config: &DicomGatewayConfig,
    permit: Option<OwnedSemaphorePermit>,
) -> Result<Response, ApiError> {
    let mut request = HeaderMap::new();
    if incoming.get_all(header::ACCEPT).iter().count() > 1
        || incoming.get_all(header::RANGE).iter().count() > 1
    {
        return Err(ApiError::bad_request(
            "A single Accept/Range header is required",
        ));
    }
    let accept = incoming.get(header::ACCEPT).cloned().unwrap_or_else(|| {
        HeaderValue::from_static("multipart/related; type=\"application/dicom\"; transfer-syntax=*")
    });
    if accept.as_bytes().len() > 4096 || accept.to_str().is_err() {
        return Err(ApiError::bad_request("Invalid DICOMweb Accept"));
    }
    request.insert(header::ACCEPT, accept);
    if let Some(charset) = incoming.get(header::ACCEPT_CHARSET) {
        request.insert(header::ACCEPT_CHARSET, charset.clone());
    }
    let range = incoming.get(header::RANGE);
    if let Some(range) = range {
        let valid = range
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("bytes="))
            .is_some_and(|v| {
                v.len() <= 45
                    && v.contains('-')
                    && v.bytes().all(|c| c.is_ascii_digit() || c == b'-')
                    && v.bytes().filter(|c| *c == b'-').count() == 1
                    && v != "-"
            });
        if !valid {
            return Err(ApiError::bad_request(
                "Only a single byte range is supported",
            ));
        }
        request.insert(header::RANGE, range.clone());
    }
    let response = client
        .stream(url, request, config.stream_seconds)
        .await
        .map_err(|_| ApiError::internal("PACS retrieval failed"))?;
    let status = response.status();
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::NOT_ACCEPTABLE | StatusCode::NOT_IMPLEMENTED
    ) || (status == StatusCode::RANGE_NOT_SATISFIABLE && range.is_some())
    {
        let message = match status {
            StatusCode::BAD_REQUEST => "PACS rejected the DICOM retrieval parameters",
            StatusCode::NOT_ACCEPTABLE => "PACS cannot provide the requested DICOM representation",
            StatusCode::NOT_IMPLEMENTED => "PACS does not implement the requested DICOM retrieval",
            _ => "Requested DICOM byte range is unavailable",
        };
        let mut result = ApiError::issues(
            status,
            octofhir_api::OperationOutcome::single("error", "not-supported", message).issue,
        )
        .into_response();
        result
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        result.headers_mut().insert(
            header::VARY,
            HeaderValue::from_static("Authorization, Accept"),
        );
        if status == StatusCode::RANGE_NOT_SATISFIABLE
            && let Some(value) = response.headers().get(header::CONTENT_RANGE)
            && value
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("bytes */"))
                .is_some_and(|v| {
                    !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit())
                })
        {
            result
                .headers_mut()
                .insert(header::CONTENT_RANGE, value.clone());
        }
        return Ok(result);
    }
    if !matches!(status, StatusCode::OK | StatusCode::PARTIAL_CONTENT)
        || (status == StatusCode::PARTIAL_CONTENT && range.is_none())
    {
        return Err(match status {
            StatusCode::NOT_FOUND => ApiError::not_found("DICOM instance is unavailable"),
            _ => ApiError::internal("PACS retrieval failed"),
        });
    }
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return Err(ApiError::internal("Encoded PACS responses are unsupported"));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .ok_or_else(|| ApiError::internal("PACS media type is missing"))?;
    if content_type.as_bytes().len() > 4096 {
        return Err(ApiError::internal("Invalid PACS media type"));
    }
    let media = content_type
        .to_str()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !(matches!(
        media.as_str(),
        "multipart/related"
            | "application/dicom"
            | "application/octet-stream"
            | "application/pdf"
            | "text/plain"
            | "text/html"
            | "text/xml"
            | "text/rtf"
    ) || matches!(
        media.as_str(),
        "image/jpeg"
            | "image/gif"
            | "image/jxl"
            | "image/png"
            | "image/jp2"
            | "image/jls"
            | "image/jph"
            | "video/mp4"
            | "video/mpeg"
            | "video/h265"
    )) {
        return Err(ApiError::internal("Unsupported PACS media type"));
    }
    if response
        .content_length()
        .is_some_and(|n| n > config.max_stream_bytes)
    {
        return Err(ApiError::bad_request(
            "DICOM object exceeds gateway stream limit",
        ));
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type);
    for key in [
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
    ] {
        if let Some(value) = response.headers().get(&key) {
            headers.insert(key, value.clone());
        }
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Accept"),
    );
    // Keep MIME boundaries and encoded DICOM bytes exact through outer compression.
    headers.insert(
        header::CONTENT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    let max = config.max_stream_bytes;
    let idle = Duration::from_secs(config.idle_seconds);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(config.stream_seconds);
    let stream = futures_util::stream::try_unfold(
        (response, permit, 0u64),
        move |(mut response, permit, read)| async move {
            let timeout = idle.min(deadline.saturating_duration_since(tokio::time::Instant::now()));
            let chunk = tokio::time::timeout(timeout, response.chunk())
                .await
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "DICOM stream timeout")
                })?
                .map_err(|_| std::io::Error::other("DICOM stream interrupted"))?;
            match chunk {
                Some(chunk) if (chunk.len() as u64) <= max.saturating_sub(read) => {
                    let next = read + chunk.len() as u64;
                    Ok(Some((chunk, (response, permit, next))))
                }
                Some(_) => Err(std::io::Error::other("DICOM stream limit exceeded")),
                None => Ok(None),
            }
        },
    );
    let mut result = Response::new(Body::from_stream(stream));
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dicom::DicomConnection, egress::EgressPolicy};
    use std::sync::Arc;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    #[tokio::test]
    async fn origin_parameter_errors_keep_status_without_remote_body_or_cookies() {
        for status in [400, 406, 501] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_raw("<html>private-origin-error</html>", "text/html")
                        .insert_header("set-cookie", "private-origin-cookie=secret"),
                )
                .mount(&server)
                .await;
            let source: DicomConnection = serde_json::from_value(serde_json::json!({
                "wado_url":server.uri(),"allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"}
            })).unwrap();
            let config: DicomGatewayConfig = serde_json::from_value(serde_json::json!({})).unwrap();
            let client =
                DicomWebClient::new(&source, Arc::new(EgressPolicy::permissive())).unwrap();
            let response = retrieve(
                &client,
                url::Url::parse(&server.uri()).unwrap(),
                &HeaderMap::new(),
                &config,
                None,
            )
            .await
            .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("private-origin"));
        }
    }
}
