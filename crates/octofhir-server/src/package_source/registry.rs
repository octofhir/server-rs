//! Registry access policy and lifetime of acquired package directories.
//!
//! # Contents
//! The existing AsyncRegistry boundary with an explicit offline branch.
//!
//! # Invariants
//! Offline managers own no remote registry client. Extracted archives remain
//! available for startup, hot reload and subsequent runtime installations.

use super::PreparedSources;
use octofhir_canonical_core::{
    FcmError, Result,
    config::PackageSpec,
    registry::{PackageDownload, PackageInfo, PackageMetadata, RegistryClient},
    traits::AsyncRegistry,
};

pub(crate) struct SourceRegistry {
    remote: Option<RegistryClient>,
    _sources: PreparedSources,
}

impl SourceRegistry {
    pub(crate) fn new(remote: Option<RegistryClient>, sources: PreparedSources) -> Self {
        Self {
            remote,
            _sources: sources,
        }
    }

    fn remote(&self, operation: &str) -> Result<&RegistryClient> {
        self.remote.as_ref().ok_or_else(|| FcmError::Generic(format!(
            "Package registry access disabled by packages.offline: {operation}. Supply an exact-version local package source and reload configuration."
        )))
    }
}

#[async_trait::async_trait]
impl AsyncRegistry for SourceRegistry {
    async fn get_package_metadata(&self, name: &str, version: &str) -> Result<PackageMetadata> {
        self.remote(&format!("metadata for {name}@{version}"))?
            .get_package_metadata(name, version)
            .await
    }
    async fn download_package(&self, spec: &PackageSpec) -> Result<PackageDownload> {
        self.remote(&format!("download {}@{}", spec.name, spec.version))?
            .download_package(spec)
            .await
    }
    async fn list_versions(&self, name: &str) -> Result<Vec<String>> {
        AsyncRegistry::list_versions(self.remote(&format!("versions for {name}"))?, name).await
    }
    async fn search_packages(&self, query: &str) -> Result<Vec<PackageInfo>> {
        AsyncRegistry::search_packages(self.remote("registry search")?, query).await
    }
}
