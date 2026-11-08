//! Sequential collection retrieval from exact authorized instance URLs.
//!
//! # Contents
//! Multipart flattening, bounded part headers and a shared stream lifetime/budget.
//! # Invariants
//! Never request a PACS study/series export: new instances cannot join the response.
//! Admission captures the primary-authorized manifest for this bounded request.
//! Each PACS GET freshly verifies its SOP tuple and the admitted Patient identity.
//! Backpressure and cancellation propagate without a worker task or byte storage.
use super::{
    DicomGatewayConfig,
    access::{self, Instance},
    metadata, missing,
    negotiation::Preferences,
    stream,
};
use crate::dicom::{DicomConnection, client::DicomWebClient};
use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use futures_util::StreamExt;
use octofhir_api::ApiError;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use tokio::sync::OwnedSemaphorePermit;

fn io() -> std::io::Error {
    std::io::Error::other("DICOM collection retrieval is incomplete")
}
fn budget(bytes: Bytes, remaining: &mut u64) -> Result<Bytes, std::io::Error> {
    *remaining = remaining.checked_sub(bytes.len() as u64).ok_or_else(io)?;
    Ok(bytes)
}
pub(super) struct Collection {
    pub source: DicomConnection,
    pub patient: serde_json::Value,
    pub gateway: String,
    pub client: DicomWebClient,
    pub instances: Vec<Instance>,
    pub bulk: std::collections::BTreeMap<String, Vec<String>>,
    pub suffix: Vec<String>,
    pub query: std::collections::BTreeMap<String, String>,
    pub preferences: Preferences,
}
struct Part {
    instance: Instance,
    token: Option<String>,
}
impl Part {
    fn location(&self, connection: &str) -> String {
        let mut value = self.instance.gateway(connection);
        if let Some(token) = &self.token {
            value.push_str(&format!("/bulkdata/{token}"));
        }
        value
    }
}
impl Collection {
    async fn open(
        &self,
        part: &Part,
        headers: &HeaderMap,
        config: &DicomGatewayConfig,
    ) -> Result<Response, ApiError> {
        let instance = &part.instance;
        let (dataset, base) =
            access::fresh(&self.client, &self.source, instance, &self.patient).await?;
        let mut path = instance.path();
        path.extend(self.suffix.iter().map(String::as_str));
        let mut url = if let Some(token) = &part.token {
            match metadata::payload(
                &dataset,
                token,
                &base,
                &self.client.resource_url(&[]).map_err(|_| missing())?,
            )? {
                metadata::Payload::Uri(url) => url,
                metadata::Payload::Inline(bytes) => {
                    let mut preferences =
                        Preferences::new(headers, &mut std::collections::BTreeMap::new())?;
                    preferences.validate_category(true)?;
                    return super::encoding::inline_binary(
                        bytes,
                        &preferences,
                        config.max_stream_bytes,
                    );
                }
            }
        } else {
            self.client.resource_url(&path).map_err(|_| missing())?
        };
        self.preferences.charset_upstream(&mut url);
        for (key, value) in &self.query {
            url.query_pairs_mut().append_pair(key, value);
        }
        let response = stream::retrieve(&self.client, url, headers, config, None).await?;
        if response.status() != StatusCode::OK {
            return Err(ApiError::issues(
                response.status(),
                octofhir_api::OperationOutcome::single(
                    "error",
                    "not-supported",
                    "PACS cannot provide the requested collection representation",
                )
                .issue,
            ));
        }
        Ok(response)
    }
}
pub(super) async fn retrieve(
    collection: Collection,
    config: DicomGatewayConfig,
    permit: OwnedSemaphorePermit,
) -> Result<Response, ApiError> {
    if collection.preferences.headers.contains_key(header::RANGE) {
        return Err(ApiError::issues(
            StatusCode::RANGE_NOT_SATISFIABLE,
            octofhir_api::OperationOutcome::single(
                "error",
                "not-supported",
                "Byte ranges apply to individual resources, not generated collections",
            )
            .issue,
        ));
    }
    let rendered = collection.suffix.iter().any(|s| s == "rendered");
    let selected = if rendered {
        collection.preferences.select(&[
            "multipart/related; type=\"image/jpeg\"",
            "multipart/related; type=\"image/png\"",
            "multipart/related; type=\"application/pdf\"",
            "multipart/related; type=\"text/plain\"",
        ])?
    } else if collection.suffix.iter().any(|s| s == "pixeldata") {
        collection
            .preferences
            .select(&["multipart/related; type=\"application/octet-stream\""])?
    } else {
        collection.preferences.select(&[
            "multipart/related; type=\"application/dicom\"",
            "multipart/related; type=\"application/octet-stream\"",
        ])?
    };
    let mime = selected
        .parse::<mime::Mime>()
        .map_err(|_| super::negotiation::unacceptable())?;
    let part_type = mime
        .get_param("type")
        .expect("collection type")
        .as_str()
        .to_owned();
    let headers = collection
        .preferences
        .collection_headers(&part_type, rendered)?;
    let mut requests = vec![];
    for instance in &collection.instances {
        if part_type == "application/octet-stream" {
            for token in collection.bulk.get(&instance.uid).into_iter().flatten() {
                if collection.suffix.iter().any(|s| s == "pixeldata")
                    && !matches!(token.as_str(), "7FE00010" | "7FE00008" | "7FE00009")
                {
                    continue;
                }
                requests.push(Part {
                    instance: instance.clone(),
                    token: Some(token.clone()),
                });
            }
        } else {
            requests.push(Part {
                instance: instance.clone(),
                token: None,
            });
        }
    }
    if requests.is_empty() {
        return Err(missing());
    }
    if requests.len() > 10000 {
        return Err(ApiError::bad_request(
            "DICOM collection exceeds multipart limit",
        ));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(config.stream_seconds);
    let first = tokio::time::timeout_at(deadline, collection.open(&requests[0], &headers, &config))
        .await
        .map_err(|_| ApiError::internal("DICOM collection timed out"))??;
    let boundary = format!("octofhir-{}", uuid::Uuid::new_v4().simple());
    let output_boundary = boundary.clone();
    let body: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send>,
    > = Box::pin(async_stream::try_stream! {
        let _permit=permit;let mut remaining=config.max_stream_bytes;let mut first=Some(first);let mut parts=0usize;
        for part in &requests {
            let response=if let Some(response)=first.take(){response}else{
                tokio::time::timeout_at(deadline,collection.open(part,&headers,&config)).await.map_err(|_|io())?.map_err(|_|io())?
            };
            let content_type=response.headers().get(header::CONTENT_TYPE).ok_or_else(io)?.to_str().map_err(|_|io())?.parse::<mime::Mime>().map_err(|_|io())?;
            let location=part.location(&collection.gateway);
            if content_type.type_()==mime::MULTIPART {
                let boundary=content_type.get_param("boundary").ok_or_else(io)?.as_str().to_owned();
                if boundary.is_empty()||boundary.len()>70||boundary.contains(['\r','\n']){Err(io())?;}
                let phase=Arc::new(AtomicU8::new(0));let reader_phase=phase.clone();let mut header_bytes=0usize;let mut tail=Vec::new();
                let guarded=response.into_body().into_data_stream().map(move|chunk|{
                    let chunk=chunk.map_err(|_|io())?;
                    if reader_phase.load(Ordering::Relaxed)==0 {
                        // Multer may otherwise buffer an arbitrary header before it
                        // can apply its field-size limit. Inspect at most 64 KiB.
                        let available=(65536usize.saturating_sub(header_bytes)).min(chunk.len());
                        tail.extend_from_slice(&chunk[..available]);
                        if memchr::memmem::find(&tail,b"\r\n\r\n").is_some(){reader_phase.store(1,Ordering::Relaxed);header_bytes=0;tail.clear();}
                        else {header_bytes+=chunk.len();if header_bytes>65536{return Err(io());}let keep=tail.len().saturating_sub(3);tail.drain(..keep);}
                    }else{header_bytes=0;tail.clear();}
                    Ok::<_,std::io::Error>(chunk)
                });
                let constraints=multer::Constraints::new().size_limit(multer::SizeLimit::new().whole_stream(config.max_stream_bytes).per_field(config.max_stream_bytes));
                let mut multipart=multer::Multipart::with_constraints(guarded,boundary,constraints);
                let mut count=0;
                loop {
                    phase.store(0,Ordering::Relaxed);
                    let Some(mut field)=tokio::time::timeout_at(deadline,multipart.next_field()).await.map_err(|_|io())?.map_err(|_|io())? else{break;};
                    phase.store(1,Ordering::Relaxed);count+=1;parts+=1;
                    if field.headers().iter().map(|(name,value)|name.as_str().len()+value.as_bytes().len()+4).sum::<usize>()>65536{Err(io())?;}
                    if parts>10000||(part_type=="application/dicom"&&count>1){Err(io())?;}
                    let actual=field.content_type().cloned().ok_or_else(io)?;
                    if !actual.essence_str().eq_ignore_ascii_case(&part_type){Err(io())?;}
                    let prefix=format!("--{output_boundary}\r\nContent-Type: {actual}\r\nContent-Location: {location}\r\n\r\n");
                    yield budget(Bytes::from(prefix),&mut remaining)?;
                    while let Some(chunk)=tokio::time::timeout_at(deadline,field.chunk()).await.map_err(|_|io())?.map_err(|_|io())?{yield budget(chunk,&mut remaining)?;}
                    yield budget(Bytes::from_static(b"\r\n"),&mut remaining)?;
                }
                if count==0{Err(io())?;}
            }else{
                if !content_type.essence_str().eq_ignore_ascii_case(&part_type){Err(io())?;}
                parts+=1;if parts>10000{Err(io())?;}
                let prefix=format!("--{output_boundary}\r\nContent-Type: {content_type}\r\nContent-Location: {location}\r\n\r\n");yield budget(Bytes::from(prefix),&mut remaining)?;
                let mut stream=response.into_body().into_data_stream();
                while let Some(chunk)=tokio::time::timeout_at(deadline,stream.next()).await.map_err(|_|io())?{yield budget(chunk.map_err(|_|io())?,&mut remaining)?;}
                yield budget(Bytes::from_static(b"\r\n"),&mut remaining)?;
            }
        }
        yield budget(Bytes::from(format!("--{output_boundary}--\r\n")),&mut remaining)?;
    });
    let mut response = Response::new(Body::from_stream(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&format!("{selected}; boundary={boundary}"))
            .map_err(|_| ApiError::internal("Invalid multipart representation"))?,
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
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    Ok(response)
}
