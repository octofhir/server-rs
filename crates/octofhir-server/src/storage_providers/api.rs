//! Administrator-only StorageProvider resource routes with optimistic concurrency.
//!
//! # Invariants
//! Every handler requires a current administrative user, even in anonymous mode.
//! Public payloads omit credentials. Updates/deletes require If-Match; deleting a
//! provider referenced by this deployment's configured packages is rejected.
use super::{
    StorageProvider,
    store::{self, ProviderError, WriteMode},
};
use crate::{egress::EgressPolicy, server::AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use octofhir_api::ApiError;
use octofhir_auth::middleware::AdminAuth;
use serde::Deserialize;
use serde_json::json;
impl From<ProviderError> for ApiError {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Invalid(message) => Self::bad_request(message),
            ProviderError::NotFound => Self::not_found("Storage provider is unavailable"),
            ProviderError::Conflict(message) => Self::conflict(message),
            ProviderError::RetiredSeed => Self::conflict("Provider seed was deliberately removed"),
            ProviderError::Database => Self::internal("Storage provider database operation failed"),
        }
    }
}
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/StorageProvider", get(list).post(create))
        .route(
            "/StorageProvider/{id}",
            get(read).put(update).delete(delete),
        )
        .route("/StorageProvider/{id}/_history", get(history))
}
#[derive(Deserialize)]
struct Page {
    #[serde(default = "count")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn count() -> i64 {
    100
}
fn page(page: &Page) -> Result<(), ApiError> {
    if !(1..=1000).contains(&page.limit) || page.offset < 0 {
        return Err(ApiError::bad_request(
            "limit must be 1..1000 and offset nonnegative",
        ));
    }
    Ok(())
}
fn response(provider: StorageProvider, status: StatusCode) -> Response {
    let version = provider
        .meta
        .as_ref()
        .expect("stored provider metadata")
        .version_id
        .clone();
    let mut response = (status, Json(provider)).into_response();
    response.headers_mut().insert(
        header::ETAG,
        format!("W/\"{version}\"").parse().expect("numeric version"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().expect("constant"));
    response
}
fn expected(headers: &HeaderMap) -> Result<i64, ApiError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("W/\"").or_else(|| v.strip_prefix('"')))
        .and_then(|v| v.strip_suffix('"'))
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| ApiError::bad_request("A numeric If-Match version is required"))
}
async fn list(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Query(query): Query<Page>,
) -> Result<Response, ApiError> {
    page(&query)?;
    let values = store::list(&state.db_pool, query.limit, query.offset).await?;
    let mut result=Json(json!({"resourceType":"Bundle","type":"searchset","entry":values.into_iter().map(|resource|json!({"resource":resource})).collect::<Vec<_>>()})).into_response();
    result
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().expect("constant"));
    Ok(result)
}
async fn read(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    Ok(response(
        store::read(&state.db_pool, &id).await?,
        StatusCode::OK,
    ))
}
async fn create(
    State(state): State<AppState>,
    admin: AdminAuth,
    Json(provider): Json<StorageProvider>,
) -> Result<Response, ApiError> {
    let policy =
        EgressPolicy::from_server_config(&state.config.server).map_err(ApiError::bad_request)?;
    Ok(response(
        store::save(
            &state.db_pool,
            provider,
            &admin.user_id,
            WriteMode::Create,
            &policy,
        )
        .await?,
        StatusCode::CREATED,
    ))
}
async fn update(
    State(state): State<AppState>,
    admin: AdminAuth,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(provider): Json<StorageProvider>,
) -> Result<Response, ApiError> {
    if provider.id != id {
        return Err(ApiError::bad_request("Provider id differs from path"));
    }
    let version = expected(&headers)?;
    let policy =
        EgressPolicy::from_server_config(&state.config.server).map_err(ApiError::bad_request)?;
    Ok(response(
        store::save(
            &state.db_pool,
            provider,
            &admin.user_id,
            WriteMode::Update(version),
            &policy,
        )
        .await?,
        StatusCode::OK,
    ))
}
async fn delete(
    State(state): State<AppState>,
    admin: AdminAuth,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if crate::canonical::storage_provider_referenced(&id) {
        return Err(ApiError::conflict(
            "Provider is referenced by configured IG packages; remove those references before deleting",
        ));
    }
    store::remove(&state.db_pool, &id, expected(&headers)?, &admin.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn history(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(id): Path<String>,
    Query(query): Query<Page>,
) -> Result<Response, ApiError> {
    use sqlx_core::{query::query as sql, row::Row};
    page(&query)?;
    let rows=sql("SELECT version,action,resource,actor,recorded_at FROM _storage_provider_history WHERE provider_id=$1 ORDER BY version DESC LIMIT $2 OFFSET $3").bind(id).bind(query.limit).bind(query.offset).fetch_all(state.db_pool.as_ref()).await.map_err(|_|ApiError::internal("Cannot read provider history"))?;
    let values=rows.into_iter().map(|row|json!({"version":row.get::<i64,_>("version"),"action":row.get::<String,_>("action"),"resource":row.get::<serde_json::Value,_>("resource"),"actor":row.get::<String,_>("actor"),"recordedAt":row.get::<chrono::DateTime<chrono::Utc>,_>("recorded_at")})).collect::<Vec<_>>();
    let mut result = Json(json!({"entries":values})).into_response();
    result
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().expect("constant"));
    Ok(result)
}
