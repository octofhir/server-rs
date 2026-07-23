//! Storage-backed reference resolver for FHIR reference validation.
//!
//! This module provides a `ReferenceResolver` implementation that checks
//! whether referenced resources exist in the FHIR storage, and (for
//! `targetProfile` conformance) fetches referenced resource bodies from either
//! local storage or, when enabled, over the network.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use octofhir_core::fhir_reference::{FhirReference, UnresolvableReference, parse_reference};
use octofhir_fhirschema::reference::{
    ReferenceError, ReferenceResolutionResult, ReferenceResolver, ReferenceResult,
};
use octofhir_storage::DynStorage;
use serde_json::Value;

/// Reference resolver backed by FHIR storage.
///
/// Uses the storage layer to check if referenced resources exist and to fetch
/// their bodies. External absolute-URL references are fetched over HTTP only
/// when `fetch_external` is enabled, and always subject to an SSRF guard that
/// rejects private, loopback, and link-local targets.
pub struct StorageReferenceResolver {
    storage: DynStorage,
    /// Base URL for resolving absolute references
    base_url: String,
    /// Whether to dereference external absolute-URL references over the network.
    fetch_external: bool,
    /// HTTP client for external fetches (present only when `fetch_external`).
    http: Option<reqwest::Client>,
}

impl StorageReferenceResolver {
    /// Create a new storage-backed reference resolver.
    ///
    /// # Arguments
    /// * `storage` - The FHIR storage backend
    /// * `base_url` - Base URL for the FHIR server (e.g., "http://localhost:8888/fhir")
    pub fn new(storage: DynStorage, base_url: String) -> Self {
        Self::with_options(storage, base_url, false, 5000)
    }

    /// Create a resolver with network-fetch options for `targetProfile`
    /// conformance.
    ///
    /// # Arguments
    /// * `fetch_external` - When true, external absolute-URL references are
    ///   fetched over HTTP (subject to the SSRF guard). When false, only local
    ///   storage references are dereferenced.
    /// * `timeout_ms` - Per-request timeout for external HTTP fetches.
    pub fn with_options(
        storage: DynStorage,
        base_url: String,
        fetch_external: bool,
        timeout_ms: u64,
    ) -> Self {
        let http = if fetch_external {
            reqwest::Client::builder()
                .timeout(Duration::from_millis(timeout_ms))
                // Never follow redirects: a public URL must not be able to
                // bounce the fetch to an internal address (SSRF).
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("octofhir-server/targetProfile-resolver")
                .build()
                .ok()
        } else {
            None
        };
        Self {
            storage,
            base_url,
            fetch_external,
            http,
        }
    }

    /// Parse a reference string into (resource_type, id).
    ///
    /// Returns None for references that cannot be resolved locally:
    /// - Contained references (#id)
    /// - urn:uuid: or urn:oid: references
    /// - External server references
    fn parse_reference(&self, reference: &str) -> Option<FhirReference> {
        parse_reference(reference, Some(&self.base_url)).ok()
    }

    /// Fetch a resource body from local storage by (type, id).
    async fn fetch_local(&self, parsed: &FhirReference) -> ReferenceResult<Option<Arc<Value>>> {
        match self.storage.read(&parsed.resource_type, &parsed.id).await {
            Ok(Some(stored)) => Ok(Some(Arc::new(stored.resource))),
            Ok(None) => Ok(None),
            Err(e) => Err(ReferenceError::ServiceUnavailable {
                message: e.to_string(),
            }),
        }
    }

    /// Fetch a resource body from an external absolute URL over HTTP, with an
    /// SSRF guard. Returns `Ok(None)` when disabled, blocked, or not found.
    async fn fetch_external_url(&self, url: &str) -> ReferenceResult<Option<Arc<Value>>> {
        let Some(client) = self.http.as_ref() else {
            return Ok(None);
        };

        // Parse and vet the target before issuing any request.
        let parsed = match reqwest::Url::parse(url) {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return Ok(None);
        }
        if !host_is_allowed(&parsed).await {
            tracing::warn!(
                target: "reference_resolver",
                url = %url,
                "Blocked targetProfile fetch to private/loopback/link-local address (SSRF guard)"
            );
            return Ok(None);
        }

        let resp = client
            .get(parsed)
            .header("Accept", "application/fhir+json, application/json")
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                Ok(body) => Ok(Some(Arc::new(body))),
                // Non-JSON body: treat as unresolvable rather than a hard error.
                Err(_) => Ok(None),
            },
            // 4xx/5xx: not found / unresolvable.
            Ok(_) => Ok(None),
            // Network/timeout error: transient, surface so the caller skips
            // (does not hard-fail) rather than treating as a definite mismatch.
            Err(e) => Err(ReferenceError::ServiceUnavailable {
                message: e.to_string(),
            }),
        }
    }
}

#[async_trait]
impl ReferenceResolver for StorageReferenceResolver {
    async fn resource_exists(&self, resource_type: &str, id: &str) -> ReferenceResult<bool> {
        // Existence-only query — avoids fetching the full resource JSONB.
        match self.storage.exists(resource_type, id).await {
            Ok(exists) => Ok(exists),
            Err(e) => Err(ReferenceError::ServiceUnavailable {
                message: e.to_string(),
            }),
        }
    }

    async fn resolve_reference(
        &self,
        reference: &str,
    ) -> ReferenceResult<ReferenceResolutionResult> {
        let parsed = match self.parse_reference(reference) {
            Some(r) => r,
            None => {
                // Cannot parse or external reference - skip validation
                return Ok(ReferenceResolutionResult::skipped());
            }
        };

        let exists = self
            .resource_exists(&parsed.resource_type, &parsed.id)
            .await?;

        if exists {
            Ok(ReferenceResolutionResult::found(
                parsed.resource_type,
                parsed.id,
            ))
        } else {
            Ok(ReferenceResolutionResult::not_found())
        }
    }

    async fn fetch_resource(&self, reference: &str) -> ReferenceResult<Option<Arc<Value>>> {
        match parse_reference(reference, Some(&self.base_url)) {
            // Local (relative or matching-base) reference: read from storage.
            Ok(parsed) => self.fetch_local(&parsed).await,
            // External absolute URL: fetch over HTTP if enabled + allowed.
            Err(UnresolvableReference::External(url)) => {
                if self.fetch_external {
                    self.fetch_external_url(&url).await
                } else {
                    Ok(None)
                }
            }
            // Contained / urn / invalid: not dereferenceable here.
            Err(_) => Ok(None),
        }
    }
}

/// SSRF guard: whether the URL's host is safe to fetch.
///
/// Rejects hosts that resolve to loopback, private, link-local, unspecified,
/// or otherwise internal addresses. Hostnames are resolved and *every* address
/// must be allowed, so a name pointing at an internal IP is blocked too.
async fn host_is_allowed(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    // IP literal: check directly.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return !is_blocked_ip(ip);
    }

    // Hostname: resolve and require all addresses to be allowed.
    let port = url.port_or_known_default().unwrap_or(443);
    match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => {
            let mut saw_any = false;
            for addr in addrs {
                saw_any = true;
                if is_blocked_ip(addr.ip()) {
                    return false;
                }
            }
            // No addresses resolved → cannot vet → block.
            saw_any
        }
        Err(_) => false,
    }
}

/// Whether an IP address is in a range that must never be fetched (SSRF).
fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()    // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254.0.0/16 (incl. 169.254.169.254 metadata)
                || v4.is_unspecified()// 0.0.0.0
                || v4.is_broadcast()  // 255.255.255.255
                || v4.is_documentation()
                || v4.octets()[0] == 0
                // Carrier-grade NAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // Unique local fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped/compatible: re-check the embedded v4.
                || v6.to_ipv4().is_some_and(|v4| is_blocked_ip(IpAddr::V4(v4)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_internal_ipv4() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.3.4",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "0.0.0.0",
            "100.64.0.1",
        ] {
            assert!(
                is_blocked_ip(ip.parse().unwrap()),
                "{ip} should be blocked"
            );
        }
    }

    #[test]
    fn allows_public_ipv4() {
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(
                !is_blocked_ip(ip.parse().unwrap()),
                "{ip} should be allowed"
            );
        }
    }

    #[test]
    fn blocks_internal_ipv6() {
        for ip in ["::1", "::", "fe80::1", "fc00::1", "fd12:3456::1"] {
            assert!(
                is_blocked_ip(ip.parse().unwrap()),
                "{ip} should be blocked"
            );
        }
    }
}
