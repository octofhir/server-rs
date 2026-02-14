use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Stored credentials — either Basic Auth (default) or Bearer token (OAuth)
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StoredCredentials {
    #[serde(rename = "basic")]
    Basic {
        server: String,
        username: String,
        password: String,
    },
    #[serde(rename = "bearer")]
    Bearer {
        server: String,
        access_token: String,
    },
}

impl StoredCredentials {
    pub fn server(&self) -> &str {
        match self {
            Self::Basic { server, .. } | Self::Bearer { server, .. } => server,
        }
    }
}

/// What FhirClient needs to set the Authorization header
pub enum AuthHeader {
    Basic { username: String, password: String },
    Bearer { token: String },
}

fn creds_path(profile: &str) -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".octofhir");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("credentials.{profile}.json")))
}

pub fn load_credentials(profile: &str) -> Result<Option<StoredCredentials>> {
    let path = creds_path(profile)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let creds: StoredCredentials = serde_json::from_str(&content)?;
    Ok(Some(creds))
}

pub fn save_credentials(profile: &str, creds: &StoredCredentials) -> Result<()> {
    let path = creds_path(profile)?;
    let content = serde_json::to_string_pretty(creds)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn remove_credentials(profile: &str) -> Result<bool> {
    let path = creds_path(profile)?;
    if path.exists() {
        fs::remove_file(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn to_auth_header(creds: &StoredCredentials) -> AuthHeader {
    match creds {
        StoredCredentials::Basic {
            username, password, ..
        } => AuthHeader::Basic {
            username: username.clone(),
            password: password.clone(),
        },
        StoredCredentials::Bearer { access_token, .. } => AuthHeader::Bearer {
            token: access_token.clone(),
        },
    }
}

// --- OAuth helpers (used only with --auth-flow oauth) ---

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
}

pub async fn oauth_password(
    server: &str,
    username: &str,
    password: &str,
    client_id: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server}/auth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=password&username={}&password={}&client_id={}",
            urlencoding(username),
            urlencoding(password),
            urlencoding(client_id),
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OAuth login failed (HTTP {status}): {body}");
    }

    resp.json().await.context("Failed to parse token response")
}

pub async fn oauth_client_credentials(
    server: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server}/auth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=client_credentials&client_id={}&client_secret={}",
            urlencoding(client_id),
            urlencoding(client_secret),
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OAuth login failed (HTTP {status}): {body}");
    }

    resp.json().await.context("Failed to parse token response")
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
