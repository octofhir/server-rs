//! DICOMweb media preferences shared by local metadata and upstream retrieval.
//!
//! # Contents
//! Bounded Accept parsing, query/header compatibility and weighted selection.
//! # Invariants
//! Explicit q=0 exclusions beat wildcards. Query parameters never bypass Accept.
//! UTF-8 is the local metadata encoding; unsupported charset names are ignored
//! as required by PS3.18 8.8. Upstream renderings retain their own charset.
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use octofhir_api::ApiError;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Range {
    mime: mime::Mime,
    quality: u16,
}
#[derive(Clone)]
pub(super) struct Preferences {
    data: bool,
    ranges: Vec<Range>,
    query: Vec<Range>,
    accept_query: Option<String>,
    charset_query: Option<String>,
    pub headers: HeaderMap,
}
pub(super) fn unacceptable() -> ApiError {
    ApiError::issues(
        StatusCode::NOT_ACCEPTABLE,
        octofhir_api::OperationOutcome::single(
            "error",
            "not-supported",
            "Requested DICOM representation is unavailable",
        )
        .issue,
    )
}
fn bad() -> ApiError {
    ApiError::bad_request("Invalid DICOMweb negotiation")
}
fn weight(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match whole {
        "0" => Some(if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u16>().ok()? * 10u16.pow(3 - fraction.len() as u32)
        }),
        "1" if fraction.bytes().all(|b| b == b'0') => Some(1000),
        _ => None,
    }
}
fn split(value: &str) -> Result<Vec<&str>, ApiError> {
    if value.len() > 4096 {
        return Err(bad());
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    let mut result = vec![];
    for (i, b) in value.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if quoted => escaped = true,
            b'"' => quoted = !quoted,
            b',' if !quoted => {
                result.push(value[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return Err(bad());
    }
    result.push(value[start..].trim());
    if result.len() > 64 || result.iter().any(|s| s.is_empty()) {
        return Err(bad());
    }
    Ok(result)
}
fn parse(value: &str, query: bool) -> Result<Vec<Range>, ApiError> {
    let mut result = vec![];
    for value in split(value)? {
        let Ok(mime) = value.parse::<mime::Mime>() else {
            if query {
                return Err(bad());
            } else {
                continue;
            }
        };
        if query && (mime.type_() == mime::STAR || mime.subtype() == mime::STAR) {
            return Err(bad());
        }
        let Some(quality) = mime
            .get_param("q")
            .map(|q| weight(q.as_str()))
            .unwrap_or(Some(1000))
        else {
            if query {
                return Err(bad());
            } else {
                continue;
            }
        };
        result.push(Range { mime, quality });
    }
    Ok(result)
}
fn matches(range: &mime::Mime, actual: &mime::Mime) -> bool {
    (range.type_() == mime::STAR || range.type_() == actual.type_())
        && (range.subtype() == mime::STAR || range.subtype() == actual.subtype())
        && range
            .params()
            .filter(|(key, _)| {
                key.as_str() != "q"
                    && key.as_str() != "charset"
                    && !(key.as_str() == "transfer-syntax"
                        && actual.type_() == "multipart"
                        && actual.get_param("transfer-syntax").is_none())
            })
            .all(|(key, value)| {
                actual.get_param(key).is_some_and(|v| {
                    v == value
                        || (key.as_str() == "type"
                            && v.as_str().eq_ignore_ascii_case(value.as_str()))
                        || (key.as_str() == "transfer-syntax" && value.as_str() == "*")
                })
            })
}
fn quality(ranges: &[Range], actual: &mime::Mime) -> Option<u16> {
    ranges
        .iter()
        .filter(|r| matches(&r.mime, actual))
        .max_by_key(|r| {
            (
                u8::from(r.mime.type_() != mime::STAR) + u8::from(r.mime.subtype() != mime::STAR),
                r.mime
                    .params()
                    .filter(|(k, _)| {
                        k.as_str() != "q"
                            && k.as_str() != "charset"
                            && !(k.as_str() == "transfer-syntax"
                                && actual.get_param("transfer-syntax").is_none())
                    })
                    .count(),
                r.quality,
            )
        })
        .map(|r| r.quality)
}
fn category(mime: &mime::Mime, data: bool) -> Option<bool> {
    let inner = mime
        .get_param("type")
        .map(|v| v.as_str())
        .unwrap_or(mime.essence_str())
        .to_ascii_lowercase();
    if inner.starts_with("application/dicom")
        || inner == "application/octet-stream"
        || (data && (inner.starts_with("image/") || inner.starts_with("video/")))
    {
        Some(true)
    } else if inner.starts_with("image/")
        || inner.starts_with("video/")
        || inner.starts_with("text/")
        || inner == "application/pdf"
    {
        Some(false)
    } else {
        None
    }
}
impl Preferences {
    pub fn new(
        headers: &HeaderMap,
        query: &mut BTreeMap<String, String>,
    ) -> Result<Self, ApiError> {
        let value = headers
            .get_all(header::ACCEPT)
            .iter()
            .map(|v| v.to_str().map_err(|_| bad()))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        if value.is_empty() {
            return Err(unacceptable());
        }
        let ranges = parse(&value, false)?;
        let accept_query = query.remove("accept");
        let parsed = accept_query
            .as_deref()
            .map(|v| parse(v, true))
            .transpose()?
            .unwrap_or_default();
        for entry in &parsed {
            if quality(&ranges, &entry.mime).unwrap_or(0) == 0 && entry.quality > 0 {
                return Err(bad());
            }
        }
        let charset_query = query.remove("charset");
        if charset_query
            .as_ref()
            .is_some_and(|v| v.len() > 1024 || v.contains('*'))
        {
            return Err(bad());
        }
        if headers
            .get_all(header::ACCEPT_CHARSET)
            .iter()
            .any(|v| v.as_bytes().len() > 1024 || v.to_str().is_err())
        {
            return Err(bad());
        }
        let mut normalized = headers.clone();
        normalized.insert(
            header::ACCEPT,
            HeaderValue::from_str(&value).map_err(|_| bad())?,
        );
        Ok(Self {
            data: false,
            ranges,
            query: parsed,
            accept_query,
            charset_query,
            headers: normalized,
        })
    }
    pub fn validate_category(&mut self, data: bool) -> Result<(), ApiError> {
        self.data = data;
        let mut dicom = false;
        let mut rendered = false;
        for entry in self
            .ranges
            .iter()
            .chain(&self.query)
            .filter(|r| r.quality > 0)
        {
            match category(&entry.mime, data) {
                Some(true) => dicom = true,
                Some(false) => rendered = true,
                None => {}
            }
        }
        if dicom && rendered {
            Err(bad())
        } else {
            Ok(())
        }
    }
    pub fn select<'a>(&self, candidates: &[&'a str]) -> Result<&'a str, ApiError> {
        let header_ranges = self
            .ranges
            .iter()
            .filter(|r| self.data || r.mime.get_param("transfer-syntax").is_none())
            .cloned()
            .collect::<Vec<_>>();
        let query_ranges = self
            .query
            .iter()
            .filter(|r| self.data || r.mime.get_param("transfer-syntax").is_none())
            .cloned()
            .collect::<Vec<_>>();
        for ranges in [&query_ranges, &header_ranges] {
            let selected = candidates
                .iter()
                .filter_map(|candidate| {
                    let mime = candidate.parse::<mime::Mime>().ok()?;
                    let q = quality(ranges, &mime)?;
                    (q > 0 && quality(&header_ranges, &mime).unwrap_or(0) > 0)
                        .then_some((*candidate, q))
                })
                .fold(None, |best: Option<(&str, u16)>, next| {
                    if best.is_none_or(|(_, q)| next.1 > q) {
                        Some(next)
                    } else {
                        best
                    }
                });
            if let Some((value, _)) = selected {
                return Ok(value);
            }
        }
        Err(unacceptable())
    }
    pub fn upstream(&self, url: &mut url::Url) {
        if let Some(value) = &self.accept_query {
            url.query_pairs_mut().append_pair("accept", value);
        }
        if let Some(value) = &self.charset_query {
            url.query_pairs_mut().append_pair("charset", value);
        }
    }
    pub fn collection_headers(
        &self,
        part_type: &str,
        rendered: bool,
    ) -> Result<HeaderMap, ApiError> {
        let mut headers = self.headers.clone();
        let base = if rendered {
            part_type.to_owned()
        } else {
            format!("multipart/related; type=\"{part_type}\"")
        };
        let mut values = vec![];
        if !rendered {
            let mime = base.parse::<mime::Mime>().map_err(|_| bad())?;
            let ranges = if self
                .query
                .iter()
                .any(|r| r.quality > 0 && matches(&r.mime, &mime))
            {
                &self.query
            } else {
                &self.ranges
            };
            let mut ranges = ranges
                .iter()
                .filter(|r| matches(&r.mime, &mime))
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| std::cmp::Reverse(range.quality));
            for range in ranges {
                let mut value = base.clone();
                if let Some(syntax) = range.mime.get_param("transfer-syntax") {
                    let syntax = syntax.as_str();
                    if syntax != "*" {
                        crate::dicom::validate_uid(syntax).map_err(|_| bad())?;
                    }
                    value.push_str(&format!("; transfer-syntax={syntax}"));
                }
                if range.quality < 1000 {
                    value.push_str(&format!("; q=0.{:03}", range.quality));
                }
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        if values.is_empty() {
            values.push(base);
        }
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_str(&values.join(", ")).map_err(|_| bad())?,
        );
        Ok(headers)
    }
    pub fn charset_upstream(&self, url: &mut url::Url) {
        if let Some(value) = &self.charset_query {
            url.query_pairs_mut().append_pair("charset", value);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn preferences(value: &str, query: Option<&str>) -> Result<Preferences, ApiError> {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT, value.parse().unwrap());
        let mut q = BTreeMap::new();
        if let Some(v) = query {
            q.insert("accept".into(), v.into());
        }
        let mut preferences = Preferences::new(&h, &mut q)?;
        preferences.validate_category(false)?;
        Ok(preferences)
    }
    #[test]
    fn weights_exclusions_and_query_priority() {
        let options = [
            "application/dicom+json",
            "multipart/related; type=\"application/dicom+xml\"",
        ];
        assert_eq!(
            preferences("application/dicom+json;q=0, */*;q=0.5", None)
                .unwrap()
                .select(&options)
                .unwrap(),
            options[1]
        );
        assert_eq!(
            preferences("*/*", Some(options[0]))
                .unwrap()
                .select(&options)
                .unwrap(),
            options[0]
        );
        assert!(preferences("image/jpeg", Some(options[0])).is_err());
        assert!(preferences("application/dicom+json, image/jpeg", None).is_err());
        assert!(preferences("*/*", Some("*/*")).is_err());
        assert!(
            preferences("application/not-supported", None)
                .unwrap()
                .select(&options)
                .is_err()
        );
    }
    #[test]
    fn compressed_bulk_media_are_dicom_data_for_frame_requests() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT,"multipart/related; type=\"application/octet-stream\", multipart/related; type=\"image/jpeg\"".parse().unwrap());
        let mut preferences = Preferences::new(&headers, &mut BTreeMap::new()).unwrap();
        assert!(preferences.validate_category(true).is_ok());
        assert!(preferences.validate_category(false).is_err());
        assert!(Preferences::new(&HeaderMap::new(), &mut BTreeMap::new()).is_err());
    }
    #[test]
    fn collection_transfer_syntax_preferences_keep_weights() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT,"multipart/related; type=\"application/dicom\"; transfer-syntax=1.2.840.10008.1.2.4.50; q=0.1, multipart/related; type=\"application/dicom\"; transfer-syntax=1.2.840.10008.1.2.1; q=0.9".parse().unwrap());
        let mut preferences = Preferences::new(&headers, &mut BTreeMap::new()).unwrap();
        preferences.validate_category(true).unwrap();
        let headers = preferences
            .collection_headers("application/dicom", false)
            .unwrap();
        let value = headers[header::ACCEPT].to_str().unwrap();
        assert!(
            value.find("syntax=1.2.840.10008.1.2.1;").unwrap()
                < value.find("syntax=1.2.840.10008.1.2.4.50;").unwrap()
        );
        assert!(value.contains("q=0.900"));
        assert!(value.contains("q=0.100"));
    }
}
