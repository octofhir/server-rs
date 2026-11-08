//! Authorized DICOMweb Search, Retrieve and Retrieve Capabilities gateway.
//!
//! # Contents
//! Negotiation, attested metadata, QIDO, collection streaming and base OPTIONS.
//! # Invariants
//! Every clinical request uses current primary-backed FHIR authorization. PACS
//! credentials never enlarge attested membership. Collections fetch individual
//! authorized URLs; DICOM bytes are streamed and never persisted in PostgreSQL.
//! # See also
//! `docs/src/content/docs/dicom.mdx` and `plans/dicom-integration.md`.
mod access;
mod aggregate;
mod capabilities;
mod config;
mod encoding;
mod metadata;
mod negotiation;
mod path;
mod query;
mod stream;
use super::{client::DicomWebClient, mapping};
use crate::{operations::OperationContext, server::AppState};
use axum::{
    Router,
    extract::{OriginalUri, Path, State},
    http::HeaderMap,
    response::Response,
    routing::{get, options},
};
pub use config::DicomGatewayConfig;
use octofhir_api::ApiError;
use path::{Kind, Target};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/dicomweb/{connection}", options(capabilities::options))
        .route("/dicomweb/{connection}/", options(capabilities::options))
        .route(
            "/dicomweb/{connection}/search-status/more-results",
            get(capabilities::status),
        )
        .route("/dicomweb/{connection}/{*path}", get(handle))
}
fn missing() -> ApiError {
    ApiError::not_found("DICOM study is unavailable")
}
fn rendering(query: &BTreeMap<String, String>, suffix: &[String]) -> Result<(), ApiError> {
    if !query.is_empty() && !suffix.iter().any(|s| s == "rendered" || s == "thumbnail") {
        return Err(ApiError::bad_request(
            "Query parameters require a rendered resource",
        ));
    }
    for (key, value) in query {
        if !matches!(
            key.as_str(),
            "viewport" | "window" | "quality" | "iccprofile" | "annotation"
        ) || value.len() > 1024
        {
            return Err(ApiError::bad_request("Unsupported rendering parameter"));
        }
    }
    Ok(())
}
async fn handle(
    State(state): State<AppState>,
    context: OperationContext,
    Path((connection, path)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if context.auth().is_none() {
        return Err(ApiError::forbidden(
            "DICOMweb requires an authenticated FHIR identity",
        ));
    }
    let config = state
        .config
        .dicom
        .connections
        .get(&connection)
        .ok_or_else(missing)?;
    let gateway = config.gateway.as_ref().ok_or_else(missing)?;
    let gateway_base = format!(
        "{}/dicomweb/{connection}",
        state.base_url.trim_end_matches('/')
    );
    let permit = state
        .dicom_streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::conflict("DICOM gateway capacity is busy; retry later"))?;
    let mut query = query::parse(uri.query())?;
    let mut preferences = negotiation::Preferences::new(&headers, &mut query)?;
    let target = Target::parse(&path, None)?;
    let data = !matches!(
        target.kind,
        Kind::Studies | Kind::Series | Kind::Instances | Kind::Metadata
    ) && !target
        .suffix
        .iter()
        .any(|s| s == "rendered" || s == "thumbnail");
    preferences.validate_category(data)?;
    if matches!(target.kind, Kind::Studies | Kind::Series | Kind::Instances) {
        return query::search(&state, &context, &connection, &target, query, &preferences).await;
    }
    let manifest = access::Manifest::load(&state, &context, &connection, &target).await?;
    let policy = Arc::new(
        crate::egress::EgressPolicy::from_server_config(&state.config.server)
            .map_err(|_| ApiError::internal("Invalid DICOM egress policy"))?,
    );
    let client = DicomWebClient::new(config, policy)
        .map_err(|_| ApiError::internal("PACS is unavailable"))?;
    if target.kind == Kind::Metadata {
        if !query.is_empty() {
            return Err(ApiError::bad_request(
                "Unsupported metadata query parameter",
            ));
        }
        // Negotiate before contacting PACS, including explicit q=0 exclusions.
        preferences.select(&[encoding::JSON, encoding::XML])?;
        let datasets = manifest
            .metadata(&client, config, &target, &gateway_base, true)
            .await?;
        return encoding::response(&datasets, &preferences, gateway.max_query_bytes);
    }
    rendering(&query, &target.suffix)?;
    if target.kind == Kind::Collection && !target.suffix.iter().any(|s| s == "thumbnail") {
        // Ensure the captured selection is complete before any response bytes.
        let datasets = manifest
            .metadata(&client, config, &target, &gateway_base, false)
            .await?;
        let mut bulk = BTreeMap::new();
        for dataset in &datasets {
            let uid = mapping::required(dataset, "00080018", "UI").map_err(|_| missing())?;
            bulk.insert(uid.to_owned(), metadata::tokens(dataset)?);
        }
        let limits = gateway.clone();
        return aggregate::retrieve(
            aggregate::Collection {
                source: config.clone(),
                patient: manifest.patient.resource,
                gateway: gateway_base,
                client,
                instances: manifest.instances,
                bulk,
                suffix: target.suffix,
                query,
                preferences,
            },
            limits,
            permit,
        )
        .await;
    }
    // Study/Series thumbnails use a deterministic authorized representative.
    let instance = &manifest.instances[0];
    let (dataset, metadata_url) =
        access::fresh(&client, config, instance, &manifest.patient.resource).await?;
    let token = if let Kind::Bulk(token) = &target.kind {
        Some(token.as_str())
    } else if target.suffix == ["pixeldata"] {
        Some(
            ["7FE00010", "7FE00008", "7FE00009"]
                .into_iter()
                .find(|tag| dataset.get(*tag).is_some())
                .ok_or_else(missing)?,
        )
    } else {
        None
    };
    let mut url = if let Some(token) = token {
        match metadata::payload(
            &dataset,
            token,
            &metadata_url,
            &client.resource_url(&[]).map_err(|_| missing())?,
        )? {
            metadata::Payload::Uri(url) => url,
            metadata::Payload::Inline(bytes) => {
                return encoding::inline_binary(bytes, &preferences, gateway.max_stream_bytes);
            }
        }
    } else {
        let mut path = instance.path();
        path.extend(target.suffix.iter().map(String::as_str));
        client.resource_url(&path).map_err(|_| missing())?
    };
    preferences.upstream(&mut url);
    for (key, value) in &query {
        url.query_pairs_mut().append_pair(key, value);
    }
    stream::retrieve(&client, url, &preferences.headers, gateway, Some(permit)).await
}
