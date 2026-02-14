use anyhow::{Context, Result};
use serde_json::Value;

use crate::auth::AuthHeader;

pub struct FhirClient {
    http: reqwest::Client,
    base_url: String,
    auth: Option<AuthHeader>,
}

impl FhirClient {
    pub fn new(base_url: &str, auth: Option<AuthHeader>) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            http: reqwest::Client::new(),
            base_url,
            auth,
        }
    }

    fn fhir_url(&self, path: &str) -> String {
        format!("{}/fhir/{}", self.base_url, path)
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, url);
        match &self.auth {
            Some(AuthHeader::Basic { username, password }) => {
                req = req.basic_auth(username, Some(password));
            }
            Some(AuthHeader::Bearer { token }) => {
                req = req.bearer_auth(token);
            }
            None => {}
        }
        req.header("Accept", "application/fhir+json")
    }

    pub async fn read(&self, resource_type: &str, id: &str) -> Result<Value> {
        let url = self.fhir_url(&format!("{resource_type}/{id}"));
        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn create(&self, resource_type: &str, body: &Value) -> Result<Value> {
        let url = self.fhir_url(resource_type);
        let resp = self
            .request(reqwest::Method::POST, &url)
            .header("Content-Type", "application/fhir+json")
            .json(body)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn update(&self, resource_type: &str, id: &str, body: &Value) -> Result<Value> {
        let url = self.fhir_url(&format!("{resource_type}/{id}"));
        let resp = self
            .request(reqwest::Method::PUT, &url)
            .header("Content-Type", "application/fhir+json")
            .json(body)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn delete(&self, resource_type: &str, id: &str) -> Result<()> {
        let url = self.fhir_url(&format!("{resource_type}/{id}"));
        let resp = self
            .request(reqwest::Method::DELETE, &url)
            .send()
            .await
            .context("Failed to connect to server")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE failed (HTTP {status}): {body}");
        }
        Ok(())
    }

    pub async fn search(&self, resource_type: &str, params: &[(String, String)]) -> Result<Value> {
        let url = self.fhir_url(resource_type);
        let resp = self
            .request(reqwest::Method::GET, &url)
            .query(params)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn history(&self, resource_type: &str, id: &str) -> Result<Value> {
        let url = self.fhir_url(&format!("{resource_type}/{id}/_history"));
        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn metadata(&self) -> Result<Value> {
        let url = self.fhir_url("metadata");
        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .await
            .context("Failed to connect to server")?;
        handle_response(resp).await
    }

    pub async fn health(&self) -> Result<(u16, String)> {
        let url = format!("{}/healthz", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to connect to server")?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok((status, body))
    }
}

async fn handle_response(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        if let Ok(json) = serde_json::from_str::<Value>(&body)
            && json.get("resourceType").and_then(|v| v.as_str()) == Some("OperationOutcome")
            && let Some(issues) = json.get("issue").and_then(|v| v.as_array())
        {
            let msgs: Vec<&str> = issues
                .iter()
                .filter_map(|i| i.get("diagnostics").and_then(|d| d.as_str()))
                .collect();
            if !msgs.is_empty() {
                anyhow::bail!("HTTP {status}: {}", msgs.join("; "));
            }
        }
        anyhow::bail!("HTTP {status}: {body}");
    }

    if body.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&body).context("Failed to parse response JSON")
}
