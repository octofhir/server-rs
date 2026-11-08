//! Guarded UPS-RS Requestor/Watcher HTTP transactions.
//!
//! # Invariants
//! Stable workitem UIDs make ambiguous creation recoverable by GET. The server
//! never takes a performer transaction UID or changes a workitem's state. Cancel
//! is a request; only a fresh Task Manager state becomes a FHIR transition.
use super::{config::DicomUpsConfig, model::Request};
use crate::dicom::{DicomConnection, DicomError, client::DicomWebClient};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct Client {
    inner: DicomWebClient,
    config: DicomUpsConfig,
}
impl Client {
    pub fn new(
        connection: &DicomConnection,
        policy: Arc<crate::egress::EgressPolicy>,
    ) -> Result<Self, String> {
        let config = connection.ups.as_ref().ok_or("UPS is disabled")?.clone();
        let mut source = connection.clone();
        source.wado_url = config.url.clone();
        source.bearer_token_env = config.bearer_token_env.clone();
        source.max_instances = 1;
        Ok(Self {
            inner: DicomWebClient::new(&source, policy)
                .map_err(|_| "Cannot initialize UPS client")?,
            config,
        })
    }
    pub fn url(&self, path: &[&str]) -> Result<url::Url, String> {
        self.inner
            .resource_url(path)
            .map_err(|_| "Invalid UPS service URL".into())
    }
    pub async fn retrieve(&self, uid: &str) -> Result<Option<Value>, String> {
        crate::dicom::validate_uid(uid).map_err(|_| "Invalid UPS UID")?;
        match self.inner.metadata(self.url(&["workitems", uid])?).await {
            Ok(mut rows) if rows.len() == 1 => Ok(rows.pop()),
            Ok(_) => Err("UPS retrieve requires one workitem".into()),
            Err(DicomError::Status(404)) => Ok(None),
            Err(_) => Err("UPS retrieval failed".into()),
        }
    }
    pub async fn create(
        &self,
        uid: &str,
        request: &Request,
        connection: &DicomConnection,
    ) -> Result<(), String> {
        let mut url = self.url(&["workitems"])?;
        url.query_pairs_mut().append_pair("workitem", uid);
        let (status, location) = self
            .inner
            .command(
                Method::POST,
                url.clone(),
                Some(&json!([request.dataset(connection)?])),
            )
            .await
            .map_err(|_| "UPS creation outcome is unknown; retry with the same UID")?;
        if status != 201 {
            return Err(if status == 409 {
                "UPS UID already exists; retrieve and reconcile before retry"
            } else {
                "UPS creation failed"
            }
            .into());
        }
        if let Some(location) = location {
            let location = url
                .join(&location)
                .map_err(|_| "Invalid UPS creation location")?;
            let expected = self.url(&["workitems", uid])?;
            if location != expected {
                return Err("UPS creation location differs from the persisted UID".into());
            }
        }
        Ok(())
    }
    pub async fn cancel(&self, uid: &str) -> Result<(), String> {
        let mut url = self.url(&["workitems", uid, "cancelrequest"])?;
        url.query_pairs_mut()
            .append_pair("requester", &self.config.ae_title);
        let (status, _) = self
            .inner
            .command(Method::POST, url, Some(&json!([{}])))
            .await
            .map_err(|_| "UPS cancellation outcome is unknown")?;
        if !matches!(status, 200 | 202 | 204) {
            return Err(
                "UPS cancellation request failed; reconcile current state before retry".into(),
            );
        }
        Ok(())
    }
    pub async fn subscribe(&self, prefix: &str) -> Result<(), String> {
        let mut url = self.url(&[
            "workitems",
            "1.2.840.10008.5.1.4.34.5.1",
            "subscribers",
            &self.config.ae_title,
        ])?;
        url.query_pairs_mut()
            .append_pair("deletionlock", "true")
            .append_pair("ProcedureStepLabel", &format!("{prefix}*"));
        let (status, _) = self
            .inner
            .command(Method::POST, url, None)
            .await
            .map_err(|_| "UPS filtered subscription failed")?;
        if !matches!(status, 200 | 201 | 204) {
            return Err("UPS filtered subscription failed".into());
        }
        Ok(())
    }
    pub async fn unsubscribe(&self, uid: &str) -> Result<(), String> {
        let url = self.url(&["workitems", uid, "subscribers", &self.config.ae_title])?;
        let (status, _) = self
            .inner
            .command(Method::DELETE, url, None)
            .await
            .map_err(|_| "UPS subscription release failed")?;
        if !matches!(status, 200 | 204 | 404) {
            return Err("UPS subscription release failed".into());
        }
        Ok(())
    }
}
