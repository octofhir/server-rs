//! Revocation lists that certificates name themselves: the CRL Distribution Points extension
//! (RFC 5280 §4.2.1.13). Before a chain is validated, the lists of its certificates are fetched
//! over HTTP and kept until their `nextUpdate`, so revocation is current without anyone copying
//! lists into the configuration.
//!
//! A list that cannot be fetched or read is remembered as such for a few minutes, so a provider
//! that is down is not asked again on every request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, CertificateRevocationListDer};
use time::OffsetDateTime;
use webpki::{CertRevocationList, OwnedCertRevocationList};
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::{DistributionPointName, GeneralName, ParsedExtension};
use x509_parser::prelude::FromDer;
use x509_parser::revocation_list::CertificateRevocationList;

/// How long a fetched list is kept when it gives no `nextUpdate`.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(3600);
/// The longest a list is kept, whatever its `nextUpdate` says.
const MAX_LIFETIME: Duration = Duration::from_secs(24 * 3600);
/// How long a list that could not be had is not asked for again.
const FAILURE_LIFETIME: Duration = Duration::from_secs(300);
/// Lists larger than this are not read.
const MAX_SIZE: usize = 16 * 1024 * 1024;

enum Entry {
    List {
        list: Arc<CertRevocationList<'static>>,
        until: Instant,
    },
    Failed {
        until: Instant,
    },
}

/// The lists named by certificates, fetched and cached.
pub struct DistributionPoints {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, Entry>>,
}

impl Default for DistributionPoints {
    fn default() -> Self {
        Self::new()
    }
}

impl DistributionPoints {
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be built (no TLS backend).
    #[must_use]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("octofhir/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP client"),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The http(s) URIs of the CRL Distribution Points of the certificates of `chain`.
    #[must_use]
    pub fn named_by(chain: &[CertificateDer<'_>]) -> Vec<String> {
        let mut uris = Vec::new();
        for certificate in chain {
            let Ok((_, certificate)) = X509Certificate::from_der(certificate.as_ref()) else {
                continue;
            };
            for extension in certificate.extensions() {
                let ParsedExtension::CRLDistributionPoints(points) = extension.parsed_extension()
                else {
                    continue;
                };
                for point in points.iter() {
                    let Some(DistributionPointName::FullName(names)) = &point.distribution_point
                    else {
                        continue;
                    };
                    for name in names {
                        if let GeneralName::URI(uri) = name
                            && (uri.starts_with("http://") || uri.starts_with("https://"))
                            && !uris.iter().any(|known| known == uri)
                        {
                            uris.push((*uri).to_owned());
                        }
                    }
                }
            }
        }
        uris
    }

    /// Fetches the lists `chain` names that are not cached, or no longer current.
    pub async fn fetch_for(&self, chain: &[CertificateDer<'_>]) {
        for uri in Self::named_by(chain) {
            if self.is_fresh(&uri) {
                continue;
            }
            let entry = match self.fetch(&uri).await {
                Ok((list, until)) => Entry::List {
                    list: Arc::new(list),
                    until,
                },
                Err(error) => {
                    tracing::warn!(uri, %error, "Cannot fetch a certificate revocation list");
                    Entry::Failed {
                        until: Instant::now() + FAILURE_LIFETIME,
                    }
                }
            };
            self.lock().insert(uri, entry);
        }
    }

    /// The cached, current lists `chain` names.
    #[must_use]
    pub fn lists_for(&self, chain: &[CertificateDer<'_>]) -> Vec<Arc<CertRevocationList<'static>>> {
        let now = Instant::now();
        let cache = self.lock();
        Self::named_by(chain)
            .iter()
            .filter_map(|uri| match cache.get(uri) {
                Some(Entry::List { list, until }) if *until > now => Some(list.clone()),
                _ => None,
            })
            .collect()
    }

    fn is_fresh(&self, uri: &str) -> bool {
        let now = Instant::now();
        match self.lock().get(uri) {
            Some(Entry::List { until, .. } | Entry::Failed { until }) => *until > now,
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn fetch(&self, uri: &str) -> Result<(CertRevocationList<'static>, Instant), String> {
        let response = self
            .http
            .get(uri)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?;
        if response
            .content_length()
            .is_some_and(|length| length as usize > MAX_SIZE)
        {
            return Err("the list is too large".into());
        }
        let bytes = response.bytes().await.map_err(|error| error.to_string())?;
        if bytes.len() > MAX_SIZE {
            return Err("the list is too large".into());
        }
        let der = if bytes.starts_with(b"-----BEGIN") {
            CertificateRevocationListDer::from_pem_slice(&bytes)
                .map_err(|error| error.to_string())?
                .as_ref()
                .to_vec()
        } else {
            bytes.to_vec()
        };
        let until = Instant::now() + lifetime(&der);
        let list = OwnedCertRevocationList::from_der(&der).map_err(|error| error.to_string())?;
        Ok((CertRevocationList::from(list), until))
    }
}

/// How long a list stays current: until its `nextUpdate`, within bounds.
fn lifetime(der: &[u8]) -> Duration {
    let Ok((_, list)) = CertificateRevocationList::from_der(der) else {
        return DEFAULT_LIFETIME;
    };
    let Some(next) = list.next_update() else {
        return DEFAULT_LIFETIME;
    };
    let seconds = next.timestamp() - OffsetDateTime::now_utc().unix_timestamp();
    if seconds <= 0 {
        // Already past: validation will say so; ask again soon.
        return FAILURE_LIFETIME;
    }
    Duration::from_secs(seconds as u64).min(MAX_LIFETIME)
}
