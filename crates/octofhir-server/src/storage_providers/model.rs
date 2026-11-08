//! Owned S3 provider resources and credential references shared by consumers.
//!
//! # Contents
//! Resource validation, connection addressing and redacted credential DTOs.
//!
//! # Invariants
//! The driver is S3, independent of a vendor. Secret input fields never serialize.
//! Explicit endpoint origins and final bucket hosts pass the normal egress policy.
use crate::egress::EgressPolicy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AddressingStyle {
    #[default]
    Path,
    VirtualHosted,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    S3,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProviderCredentials {
    Environment {
        #[serde(default = "access_env")]
        access_key_id_env: String,
        #[serde(default = "secret_env")]
        secret_access_key_env: String,
        session_token_env: Option<String>,
    },
    Stored {
        #[serde(default, skip_serializing)]
        access_key_id: Option<String>,
        #[serde(default, skip_serializing)]
        secret_access_key: Option<String>,
        #[serde(default, skip_serializing)]
        session_token: Option<String>,
    },
}
impl std::fmt::Debug for ProviderCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Environment {
                access_key_id_env,
                secret_access_key_env,
                session_token_env,
            } => f
                .debug_struct("Environment")
                .field("access_key_id_env", access_key_id_env)
                .field("secret_access_key_env", secret_access_key_env)
                .field("session_token_env", session_token_env)
                .finish(),
            Self::Stored { .. } => f.write_str("Stored(<redacted>)"),
        }
    }
}
impl Default for ProviderCredentials {
    fn default() -> Self {
        Self::Environment {
            access_key_id_env: access_env(),
            secret_access_key_env: secret_env(),
            session_token_env: None,
        }
    }
}
fn access_env() -> String {
    "AWS_ACCESS_KEY_ID".into()
}
fn secret_env() -> String {
    "AWS_SECRET_ACCESS_KEY".into()
}
fn resource_type() -> String {
    "StorageProvider".into()
}
fn enabled() -> bool {
    true
}
fn region() -> String {
    "us-east-1".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderMeta {
    pub version_id: String,
    pub last_updated: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageProvider {
    #[serde(default = "resource_type")]
    pub resource_type: String,
    pub id: String,
    pub name: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(rename = "type", default)]
    pub kind: ProviderKind,
    pub endpoint: Option<String>,
    #[serde(default = "region")]
    pub region: String,
    #[serde(default)]
    pub addressing_style: AddressingStyle,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub credentials: ProviderCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
}
impl StorageProvider {
    pub fn validate(&self, policy: &EgressPolicy) -> Result<(), String> {
        validate_id(&self.id)?;
        if self.resource_type != "StorageProvider" {
            return Err("Expected StorageProvider resource".into());
        }
        if self.name.as_ref().is_some_and(|v| {
            v.is_empty() || v.chars().count() > 256 || v.chars().any(char::is_control)
        }) {
            return Err("Invalid storage provider name".into());
        }
        if self.region.is_empty()
            || self.region.len() > 64
            || !self
                .region
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err("Invalid S3 signing region".into());
        }
        policy.check_url(&self.endpoint_url()?)?;
        match &self.credentials {
            ProviderCredentials::Environment {
                access_key_id_env,
                secret_access_key_env,
                session_token_env,
            } => {
                for name in [access_key_id_env, secret_access_key_env]
                    .into_iter()
                    .chain(session_token_env.as_ref())
                {
                    if name.is_empty()
                        || name.len() > 128
                        || name.as_bytes()[0].is_ascii_digit()
                        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    {
                        return Err(
                            "Credentials must reference valid environment variable names".into(),
                        );
                    }
                }
            }
            ProviderCredentials::Stored {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                if access_key_id.is_some() != secret_access_key.is_some()
                    || session_token.is_some() && access_key_id.is_none()
                {
                    return Err(
                        "Rotate stored credentials with both accessKeyId and secretAccessKey"
                            .into(),
                    );
                }
                if [access_key_id, secret_access_key, session_token]
                    .into_iter()
                    .flatten()
                    .any(|v| {
                        v.trim().is_empty() || v.len() > 16384 || v.chars().any(char::is_control)
                    })
                {
                    return Err("Invalid stored S3 credential input".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn endpoint_url(&self) -> Result<url::Url, String> {
        let endpoint = self.endpoint.clone().unwrap_or_else(|| {
            let suffix = if self.region.starts_with("cn-") {
                "amazonaws.com.cn"
            } else {
                "amazonaws.com"
            };
            format!("https://s3.{}.{suffix}", self.region)
        });
        let url = url::Url::parse(&endpoint).map_err(|_| "Invalid S3 endpoint")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || (url.scheme() == "http" && !self.allow_http)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("S3 endpoint must be an HTTP(S) origin; HTTP requires allowHttp, credentials/path/query/fragment are forbidden".into());
        }
        if self.addressing_style == AddressingStyle::VirtualHosted
            && !matches!(url.host(), Some(url::Host::Domain(_)))
        {
            return Err(
                "virtual_hosted addressing requires a DNS endpoint; use path for IP endpoints"
                    .into(),
            );
        }
        Ok(url)
    }
    pub(crate) fn object_url(
        &self,
        bucket: &str,
        key: &str,
        version: Option<&str>,
    ) -> Result<url::Url, String> {
        let mut url = self.endpoint_url()?;
        if self.addressing_style == AddressingStyle::VirtualHosted {
            let host = format!("{bucket}.{}", url.host_str().ok_or("Missing S3 hostname")?);
            url.set_host(Some(&host))
                .map_err(|_| "Invalid virtual-hosted bucket hostname")?;
        }
        {
            let mut path = url.path_segments_mut().map_err(|_| "Invalid S3 endpoint")?;
            path.clear();
            if self.addressing_style == AddressingStyle::Path {
                path.push(bucket);
            }
            path.extend(key.split('/'));
        }
        if let Some(version) = version {
            url.query_pairs_mut().append_pair("versionId", version);
        }
        Ok(url)
    }
}
pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    if matches!(id, "." | "..")
        || id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err("Provider id must contain 1..64 ASCII letters, digits, -, _ or .".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider(endpoint: Option<&str>, region: &str, style: AddressingStyle) -> StorageProvider {
        let mut provider: StorageProvider =
            serde_json::from_value(serde_json::json!({"id":"account","region":region})).unwrap();
        provider.endpoint = endpoint.map(Into::into);
        provider.addressing_style = style;
        provider
    }
    #[test]
    fn s3_addressing_is_independent_of_vendor_and_encodes_keys_once() {
        for (endpoint, region, style, expected) in [
            (
                None,
                "eu-west-1",
                AddressingStyle::VirtualHosted,
                "https://private-igs.s3.eu-west-1.amazonaws.com/private/a%20b+%25.tgz",
            ),
            (
                None,
                "cn-north-1",
                AddressingStyle::Path,
                "https://s3.cn-north-1.amazonaws.com.cn/private-igs/private/a%20b+%25.tgz",
            ),
            (
                Some("https://account.r2.cloudflarestorage.com"),
                "auto",
                AddressingStyle::Path,
                "https://account.r2.cloudflarestorage.com/private-igs/private/a%20b+%25.tgz",
            ),
            (
                Some("https://s3.us-west-004.backblazeb2.com"),
                "us-west-004",
                AddressingStyle::VirtualHosted,
                "https://private-igs.s3.us-west-004.backblazeb2.com/private/a%20b+%25.tgz",
            ),
            (
                Some("https://objects.example.org:9443"),
                "garage",
                AddressingStyle::Path,
                "https://objects.example.org:9443/private-igs/private/a%20b+%25.tgz",
            ),
        ] {
            let provider = provider(endpoint, region, style);
            assert_eq!(
                provider
                    .object_url("private-igs", "private/a b+%.tgz", None)
                    .unwrap()
                    .as_str(),
                expected
            );
        }
    }
    #[test]
    fn stored_secret_inputs_never_serialize_or_format_for_debug() {
        let value = serde_json::json!({"id":"account","credentials":{"type":"stored","accessKeyId":"secret-access-marker","secretAccessKey":"secret-key-marker","sessionToken":"secret-token-marker"}});
        let provider: StorageProvider = serde_json::from_value(value).unwrap();
        for output in [
            serde_json::to_string(&provider).unwrap(),
            format!("{provider:?}"),
        ] {
            for secret in [
                "secret-access-marker",
                "secret-key-marker",
                "secret-token-marker",
            ] {
                assert!(!output.contains(secret));
            }
        }
    }
    #[test]
    fn origins_and_virtual_hosted_ip_endpoints_are_rejected() {
        for endpoint in [
            "https://user:pass@objects.example",
            "https://objects.example/base",
            "https://objects.example?token=private",
            "https://objects.example#fragment",
            "file:///local",
        ] {
            assert!(
                provider(Some(endpoint), "auto", AddressingStyle::Path)
                    .endpoint_url()
                    .is_err()
            );
        }
        assert!(
            provider(
                Some("http://127.0.0.1:9000"),
                "us-east-1",
                AddressingStyle::VirtualHosted
            )
            .endpoint_url()
            .is_err()
        );
        for id in [".", "..", "../other", "name/other", ""] {
            assert!(validate_id(id).is_err());
        }
    }
}
