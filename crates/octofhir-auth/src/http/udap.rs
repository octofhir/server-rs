//! UDAP discovery and dynamic client registration.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use crate::storage::{ClientStorage, JtiStorage};
use crate::types::{Client, UdapBinding};
use crate::udap::statement::{Registration, RegistrationError, SoftwareStatement, client_id};
use crate::udap::{MAX_ACCESS_TOKEN_LIFETIME_SECONDS, Trust, TrustError, UdapMetadata, Verified};

/// What the UDAP endpoints share.
#[derive(Clone)]
pub struct UdapState {
    pub trust: Arc<Trust>,
    pub metadata: Arc<UdapMetadata>,
    pub clients: Arc<dyn ClientStorage>,
    pub jti: Arc<dyn JtiStorage>,
    /// The registration endpoint: the audience of a software statement.
    pub registration_url: String,
    /// What a registration can be granted.
    pub scopes: Arc<[String]>,
    /// The lifetime of access tokens in this server, capped for these clients.
    pub access_token_lifetime: i64,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryQuery {
    community: Option<String>,
}

/// `GET {base}/.well-known/udap`. An unknown community gets 204: no UDAP workflows there.
pub async fn udap_metadata_handler(
    State(state): State<UdapState>,
    Query(query): Query<DiscoveryQuery>,
) -> Response {
    if let Some(community) = &query.community
        && !state.trust.has_community(community)
    {
        return StatusCode::NO_CONTENT.into_response();
    }
    match state.metadata.document() {
        Ok(document) => Json(document).into_response(),
        Err(error) => {
            tracing::error!(%error, "Failed to sign the UDAP metadata");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RegistrationRequest {
    software_statement: String,
    #[serde(default)]
    udap: Option<String>,
}

fn refused(error: RegistrationError) -> Response {
    (StatusCode::BAD_REQUEST, Json(error)).into_response()
}

/// RFC 7591 metadata of the answer. Nothing unset, none of the JWT claims.
fn answer(
    client_id: &str,
    request: &str,
    statement: &SoftwareStatement,
    scopes: &[String],
) -> Value {
    let mut body = Map::new();
    body.insert("client_id".into(), json!(client_id));
    body.insert("software_statement".into(), json!(request));
    body.insert("grant_types".into(), json!(statement.grant_types));
    if !statement.grant_types.is_empty() {
        body.insert("client_name".into(), json!(statement.client_name));
        body.insert(
            "token_endpoint_auth_method".into(),
            json!(statement.token_endpoint_auth_method),
        );
        body.insert("scope".into(), json!(scopes.join(" ")));
    }
    if !statement.redirect_uris.is_empty() {
        body.insert("redirect_uris".into(), json!(statement.redirect_uris));
    }
    if let Some(types) = &statement.response_types {
        body.insert("response_types".into(), json!(types));
    }
    if !statement.contacts.is_empty() {
        body.insert("contacts".into(), json!(statement.contacts));
    }
    if let Some(logo) = &statement.logo_uri {
        body.insert("logo_uri".into(), json!(logo));
    }
    Value::Object(body)
}

fn client(
    id: String,
    community: String,
    statement: &SoftwareStatement,
    scopes: Vec<String>,
    lifetime: i64,
) -> Client {
    Client {
        client_id: id,
        client_secret: None,
        name: statement.client_name.clone().unwrap_or_default(),
        description: None,
        grant_types: statement.grants(),
        redirect_uris: statement.redirect_uris.clone(),
        post_logout_redirect_uris: Vec::new(),
        scopes,
        confidential: true,
        active: true,
        access_token_lifetime: Some(lifetime.min(MAX_ACCESS_TOKEN_LIFETIME_SECONDS)),
        refresh_token_lifetime: None,
        pkce_required: Some(true),
        allowed_origins: Vec::new(),
        jwks: None,
        jwks_uri: None,
        udap: Some(UdapBinding {
            subject: statement.iss.clone(),
            community,
        }),
    }
}

/// `POST /auth/udap/register`: registers, changes or cancels an application.
pub async fn udap_register_handler(
    State(state): State<UdapState>,
    Json(request): Json<RegistrationRequest>,
) -> Response {
    if request.udap.as_deref() != Some("1") {
        return refused(RegistrationError::metadata("udap has to be 1"));
    }
    let verified: Verified<SoftwareStatement> = match state
        .trust
        .verify(&request.software_statement, &state.registration_url)
        .await
    {
        Ok(verified) => verified,
        Err(error @ TrustError::Untrusted(_)) => {
            return refused(RegistrationError::unapproved(error.to_string()));
        }
        Err(error) => return refused(RegistrationError::statement(error.to_string())),
    };
    let statement = &verified.claims;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let registration = match statement.validate(&verified.subject_uris, &state.scopes, now) {
        Ok(registration) => registration,
        Err(error) => return refused(error),
    };

    let expires = OffsetDateTime::from_unix_timestamp(statement.exp)
        .unwrap_or_else(|_| OffsetDateTime::now_utc());
    let replay_key = format!("udap:{}:{}", statement.iss, statement.jti);
    match state.jti.mark_used(&replay_key, expires).await {
        Ok(true) => {}
        Ok(false) => return refused(RegistrationError::statement("jti was already used")),
        Err(error) => {
            tracing::error!(%error, "UDAP replay check failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let id = client_id(&verified.community, &statement.iss);
    let existing = match state.clients.find_by_client_id(&id).await {
        Ok(existing) => existing,
        Err(error) => {
            tracing::error!(%error, "UDAP registration lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let stored = match (registration, existing) {
        (Registration::Cancel, None) => {
            return refused(RegistrationError::metadata(
                "There is no registration to cancel",
            ));
        }
        (Registration::Cancel, Some(_)) => state
            .clients
            .delete(&id)
            .await
            .map(|()| (StatusCode::OK, Vec::new())),
        (Registration::Register { scopes }, existing) => {
            let updated = existing.is_some();
            let record = client(
                id.clone(),
                verified.community.clone(),
                statement,
                scopes.clone(),
                state.access_token_lifetime,
            );
            let result = if updated {
                state.clients.update(&id, &record).await
            } else {
                state.clients.create(&record).await
            };
            result.map(|_| {
                let status = if updated {
                    StatusCode::OK
                } else {
                    StatusCode::CREATED
                };
                (status, scopes)
            })
        }
    };
    match stored {
        Ok((status, scopes)) => (
            status,
            Json(answer(&id, &request.software_statement, statement, &scopes)),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "UDAP registration failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
