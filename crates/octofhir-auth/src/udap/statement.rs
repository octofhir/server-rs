//! The software statement of a registration request.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{MAX_CLOCK_SKEW_SECONDS, MAX_TOKEN_LIFETIME_SECONDS};
use crate::types::{Client, GrantType};

/// A refused registration, with the code of RFC 7591 or the UDAP profile.
#[derive(Debug, Clone, Serialize)]
pub struct RegistrationError {
    pub error: &'static str,
    pub error_description: String,
}

impl RegistrationError {
    /// A value in the registration is wrong.
    #[must_use]
    pub fn metadata(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_client_metadata",
            error_description: description.into(),
        }
    }

    /// The statement is malformed or its signature does not verify.
    #[must_use]
    pub fn statement(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_software_statement",
            error_description: description.into(),
        }
    }

    /// The statement is sound, but the sender is not trusted.
    #[must_use]
    pub fn unapproved(description: impl Into<String>) -> Self {
        Self {
            error: "unapproved_software_statement",
            error_description: description.into(),
        }
    }
}

/// Claims of a software statement.
#[derive(Debug, Clone, Deserialize)]
pub struct SoftwareStatement {
    pub iss: String,
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    pub jti: String,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub grant_types: Vec<String>,
    #[serde(default)]
    pub response_types: Option<Vec<String>>,
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub logo_uri: Option<String>,
    #[serde(default)]
    pub contacts: Vec<String>,
}

/// What a valid statement asks for.
#[derive(Debug, Clone)]
pub enum Registration {
    /// To be registered, with these scopes granted.
    Register { scopes: Vec<String> },
    /// `grant_types` is empty: the registration ends.
    Cancel,
}

/// The client id of an application in a community. Derived, so a second statement from the
/// same `iss` finds its registration, and another community can never overwrite it.
#[must_use]
pub fn client_id(community: &str, iss: &str) -> String {
    let digest = Sha256::digest(format!("{community}\n{iss}").as_bytes());
    format!("udap-{}", hex::encode(&digest[..16]))
}

fn https(uri: &str) -> bool {
    uri.strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty())
}

impl SoftwareStatement {
    /// Checks the statement as the registration profile asks, against `subject_uris` of the
    /// certificate and the scopes this server offers.
    ///
    /// # Errors
    ///
    /// Returns the [`RegistrationError`] to send back.
    pub fn validate(
        &self,
        subject_uris: &[String],
        offered_scopes: &[String],
        now: i64,
    ) -> Result<Registration, RegistrationError> {
        if self.iss != self.sub {
            return Err(RegistrationError::statement("sub has to equal iss"));
        }
        if !subject_uris.contains(&self.iss) {
            return Err(RegistrationError::unapproved(
                "iss is not a URI in the Subject Alternative Name of the certificate",
            ));
        }
        if self.exp - self.iat > MAX_TOKEN_LIFETIME_SECONDS {
            return Err(RegistrationError::statement(
                "exp is more than 5 minutes after iat",
            ));
        }
        if self.iat > now + MAX_CLOCK_SKEW_SECONDS {
            return Err(RegistrationError::statement("iat is in the future"));
        }
        if self.jti.is_empty() {
            return Err(RegistrationError::statement("jti is required"));
        }
        if self.grant_types.is_empty() {
            return Ok(Registration::Cancel);
        }
        self.validate_grants()?;
        if self.token_endpoint_auth_method.as_deref() != Some("private_key_jwt") {
            return Err(RegistrationError::metadata(
                "token_endpoint_auth_method has to be private_key_jwt",
            ));
        }
        if self.client_name.as_deref().is_none_or(str::is_empty) {
            return Err(RegistrationError::metadata("client_name is required"));
        }
        if !self.contacts.iter().any(|contact| {
            contact
                .strip_prefix("mailto:")
                .is_some_and(|address| address.contains('@'))
        }) {
            return Err(RegistrationError::metadata(
                "contacts needs an email address as a mailto URI",
            ));
        }
        let requested = self.scope.as_deref().unwrap_or_default();
        let scopes: Vec<String> = requested
            .split_whitespace()
            .filter(|scope| Client::scope_is_covered(offered_scopes, scope))
            .map(str::to_owned)
            .collect();
        if scopes.is_empty() {
            return Err(RegistrationError::metadata(
                "None of the requested scopes is available",
            ));
        }
        Ok(Registration::Register { scopes })
    }

    fn validate_grants(&self) -> Result<(), RegistrationError> {
        let code = self.has_grant("authorization_code");
        if code == self.has_grant("client_credentials") {
            return Err(RegistrationError::metadata(
                "grant_types has to hold authorization_code or client_credentials, not both",
            ));
        }
        if !code && self.has_grant("refresh_token") {
            return Err(RegistrationError::metadata(
                "refresh_token goes only with authorization_code",
            ));
        }
        if let Some(other) = self.grant_types.iter().find(|grant| {
            !matches!(
                grant.as_str(),
                "authorization_code" | "client_credentials" | "refresh_token"
            )
        }) {
            return Err(RegistrationError::metadata(format!(
                "Grant type {other} is not supported"
            )));
        }
        if !code {
            if self.response_types.is_some() {
                return Err(RegistrationError::metadata(
                    "response_types goes only with authorization_code",
                ));
            }
            return Ok(());
        }
        if self.response_types.as_deref() != Some(&["code".to_owned()][..]) {
            return Err(RegistrationError::metadata(
                "response_types has to be [\"code\"]",
            ));
        }
        if self.redirect_uris.is_empty() || !self.redirect_uris.iter().all(|uri| https(uri)) {
            return Err(RegistrationError::metadata(
                "redirect_uris needs at least one URI, all https",
            ));
        }
        let logo = self.logo_uri.as_deref().unwrap_or_default();
        let path = logo.split(['?', '#']).next().unwrap_or_default();
        if !https(logo)
            || ![".png", ".jpg", ".jpeg", ".gif"]
                .iter()
                .any(|suffix| path.to_ascii_lowercase().ends_with(suffix))
        {
            return Err(RegistrationError::metadata(
                "logo_uri has to be an https URI of a PNG, JPG or GIF",
            ));
        }
        Ok(())
    }

    fn has_grant(&self, grant: &str) -> bool {
        self.grant_types.iter().any(|name| name == grant)
    }

    /// The grant types as the server knows them.
    #[must_use]
    pub fn grants(&self) -> Vec<GrantType> {
        let mut grants = Vec::with_capacity(self.grant_types.len());
        if self.has_grant("authorization_code") {
            grants.push(GrantType::AuthorizationCode);
        }
        if self.has_grant("client_credentials") {
            grants.push(GrantType::ClientCredentials);
        }
        if self.has_grant("refresh_token") {
            grants.push(GrantType::RefreshToken);
        }
        grants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const APP: &str = "https://app.example.org/";

    fn offered() -> Vec<String> {
        vec!["openid".to_string(), "user/*.rs".to_string()]
    }

    fn statement(changes: Value) -> SoftwareStatement {
        let mut claims = json!({
            "iss": APP,
            "sub": APP,
            "exp": 1300,
            "iat": 1000,
            "jti": "1",
            "client_name": "App",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "private_key_jwt",
            "scope": "openid user/Patient.rs",
            "redirect_uris": ["https://app.example.org/callback"],
            "logo_uri": "https://app.example.org/logo.png",
            "contacts": ["mailto:ops@example.org"],
        });
        for (key, value) in changes.as_object().unwrap() {
            claims[key] = value.clone();
        }
        serde_json::from_value(claims).unwrap()
    }

    fn check(changes: Value) -> Result<Registration, RegistrationError> {
        statement(changes).validate(&[APP.to_string()], &offered(), 1000)
    }

    fn refused(changes: Value) -> &'static str {
        check(changes).unwrap_err().error
    }

    #[test]
    fn an_authorization_code_registration_passes() {
        let Registration::Register { scopes } = check(json!({})).unwrap() else {
            panic!("expected a registration");
        };
        assert_eq!(scopes, ["openid", "user/Patient.rs"]);
    }

    #[test]
    fn the_authorization_code_elements_are_required() {
        for changes in [
            json!({"response_types": ["token"]}),
            json!({"redirect_uris": []}),
            json!({"redirect_uris": ["http://app.example.org/callback"]}),
            json!({"logo_uri": "http://app.example.org/logo.png"}),
            json!({"logo_uri": "https://app.example.org/logo.svg"}),
        ] {
            assert_eq!(
                refused(changes.clone()),
                "invalid_client_metadata",
                "{changes}"
            );
        }
    }

    #[test]
    fn refresh_tokens_go_only_with_authorization_code() {
        let changes = json!({
            "grant_types": ["client_credentials", "refresh_token"],
            "response_types": null,
        });
        assert_eq!(refused(changes), "invalid_client_metadata");
    }

    #[test]
    fn response_types_go_only_with_authorization_code() {
        let changes = json!({"grant_types": ["client_credentials"]});
        assert_eq!(refused(changes), "invalid_client_metadata");
    }

    #[test]
    fn nothing_outside_the_offer_is_granted() {
        let Registration::Register { scopes } =
            check(json!({"scope": "user/Patient.cruds user/Patient.r"})).unwrap()
        else {
            panic!("expected a registration");
        };
        assert_eq!(scopes, ["user/Patient.r"]);
        assert_eq!(
            refused(json!({"scope": "system/*.cruds"})),
            "invalid_client_metadata"
        );
    }

    #[test]
    fn the_statement_itself_has_rules() {
        assert_eq!(
            refused(json!({"sub": "https://other.example.org/"})),
            "invalid_software_statement"
        );
        assert_eq!(refused(json!({"exp": 1301})), "invalid_software_statement");
        assert_eq!(
            refused(json!({"iat": 1100, "exp": 1200})),
            "invalid_software_statement"
        );
        assert_eq!(
            refused(
                json!({"iss": "https://other.example.org/", "sub": "https://other.example.org/"})
            ),
            "unapproved_software_statement"
        );
    }

    #[test]
    fn empty_grant_types_cancel_the_registration() {
        assert!(matches!(
            check(json!({"grant_types": []})).unwrap(),
            Registration::Cancel
        ));
    }

    #[test]
    fn the_client_id_differs_by_community() {
        assert_eq!(client_id("urn:a", APP), client_id("urn:a", APP));
        assert_ne!(client_id("urn:a", APP), client_id("urn:b", APP));
    }
}
