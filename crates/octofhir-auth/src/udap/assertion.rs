//! Authentication of a registered application at the token endpoint.

use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;

use super::trust::{Trust, TrustError};
use super::{B2B_EXTENSION, MAX_CLOCK_SKEW_SECONDS, MAX_TOKEN_LIFETIME_SECONDS};
use crate::AuthError;
use crate::types::{Client, UdapBinding};

/// Claims of an Authentication Token.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthenticationToken {
    pub iss: String,
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    pub jti: String,
    #[serde(default)]
    pub extensions: Option<Value>,
}

/// The HL7 B2B Authorization Extension Object.
#[derive(Debug, Clone, Deserialize)]
pub struct B2bExtension {
    pub version: String,
    pub organization_id: String,
    pub purpose_of_use: Vec<String>,
    #[serde(default)]
    pub subject_name: Option<String>,
    #[serde(default)]
    pub organization_name: Option<String>,
}

/// Validates the token of a client registered through UDAP: the chain has to lead to the
/// community of the registration and the certificate has to name the registered application.
///
/// # Errors
///
/// Trust failures are `invalid_client`; a bad signature and a missing `udap` parameter or
/// extension are `invalid_request`, as the profile sets out.
pub async fn authenticate(
    trust: &Trust,
    client: &Client,
    binding: &UdapBinding,
    assertion: &str,
    token_endpoint: &str,
    udap_parameter: Option<&str>,
    grant_type: &str,
) -> Result<AuthenticationToken, AuthError> {
    if udap_parameter != Some("1") {
        return Err(AuthError::invalid_request("udap has to be 1"));
    }
    let verified = trust
        .verify::<AuthenticationToken>(assertion, token_endpoint)
        .await
        .map_err(|error| match error {
            TrustError::Malformed(_) | TrustError::Signature(_) => {
                AuthError::invalid_request(error.to_string())
            }
            TrustError::Untrusted(_) | TrustError::Claims(_) => {
                AuthError::invalid_client(error.to_string())
            }
        })?;
    let token = verified.claims;
    if token.iss != client.client_id || token.sub != client.client_id {
        return Err(AuthError::invalid_client(
            "iss and sub have to be client_id",
        ));
    }
    if token.exp - token.iat > MAX_TOKEN_LIFETIME_SECONDS {
        return Err(AuthError::invalid_client(
            "The token lives longer than 5 minutes",
        ));
    }
    // A future iat would stretch the lifetime check above past its meaning.
    if token.iat > OffsetDateTime::now_utc().unix_timestamp() + MAX_CLOCK_SKEW_SECONDS {
        return Err(AuthError::invalid_client("iat is in the future"));
    }
    if verified.community != binding.community {
        return Err(AuthError::invalid_client(
            "The certificate belongs to another trust community",
        ));
    }
    if !verified.subject_uris.contains(&binding.subject) {
        return Err(AuthError::invalid_client(
            "The certificate does not name the registered application",
        ));
    }
    if grant_type == "client_credentials" {
        let extension = b2b(&token)?;
        tracing::info!(
            client_id = %client.client_id,
            organization_id = %extension.organization_id,
            purpose_of_use = ?extension.purpose_of_use,
            "B2B authorization"
        );
    }
    Ok(token)
}

/// The required extension of a client credentials request.
fn b2b(token: &AuthenticationToken) -> Result<B2bExtension, AuthError> {
    let value = token
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.get(B2B_EXTENSION))
        .ok_or_else(|| AuthError::invalid_request("The hl7-b2b extension is required"))?;
    let extension: B2bExtension = serde_json::from_value(value.clone())
        .map_err(|error| AuthError::invalid_request(format!("Invalid hl7-b2b: {error}")))?;
    if extension.version != "1" {
        return Err(AuthError::invalid_request("hl7-b2b version has to be 1"));
    }
    if !extension.organization_id.contains(':') {
        return Err(AuthError::invalid_request(
            "hl7-b2b organization_id has to be a URI",
        ));
    }
    if extension.purpose_of_use.is_empty() {
        return Err(AuthError::invalid_request(
            "hl7-b2b purpose_of_use needs at least one code",
        ));
    }
    Ok(extension)
}
