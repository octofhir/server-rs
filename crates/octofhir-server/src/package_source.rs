//! Acquire published FHIR packages from local sources and pinned S3 objects.
//!
//! # Contents
//! Manifest validation, offline dependency closure and canonical-manager overrides.
//! Archive extraction, S3 acquisition and registry policy live in sibling modules.
//!
//! # Invariants
//! Inputs are never modified. All sources use the canonical manager's normal
//! installation path. Offline acquisition only reads configured sources, installed
//! packages and exact-version cached archives. Registry-offline mode still permits
//! explicitly configured S3 objects through their guarded acquisition path.

mod archive;
mod registry;
mod s3;
pub use s3::S3PackageSource;
#[cfg(test)]
mod tests;

use crate::{canonical::LoadedPackage, config::PackageSpec};
use octofhir_canonical_core::{
    config::LocalPackageSpec,
    package::{PackageIndex, PackageManifest},
};
use octofhir_core::fhir::FhirVersion;
pub(crate) use registry::SourceRegistry;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_PACKAGES: usize = 4096;

/// Owns extracted directories for as long as the canonical manager can use them.
#[derive(Default)]
pub(crate) struct PreparedSources {
    packages: BTreeMap<(String, String), (LocalPackageSpec, PackageManifest)>,
    configured: BTreeMap<usize, LocalPackageSpec>,
    workspaces: Vec<tempfile::TempDir>,
}

impl PreparedSources {
    pub(crate) fn from_config(specs: &[PackageSpec], release: FhirVersion) -> Result<Self, String> {
        let mut sources = Self::default();
        for (index, spec) in specs.iter().enumerate() {
            let PackageSpec::Table {
                id,
                version,
                path: Some(path),
                url,
            } = spec
            else {
                continue;
            };
            if url.is_some() {
                return Err("Package source must select either path or url".into());
            }
            let source = sources.acquire(
                Path::new(path),
                id.as_deref(),
                version.as_deref(),
                Some(release),
            )?;
            sources.configured.insert(index, source);
        }
        Ok(sources)
    }

    pub(crate) async fn load(
        specs: Vec<PackageSpec>,
        release: FhirVersion,
        cache: PathBuf,
        policy: std::sync::Arc<crate::egress::EgressPolicy>,
        providers: &sqlx_postgres::PgPool,
    ) -> Result<Self, String> {
        // Validate all S3 sources before sending any credentials or requests.
        for spec in &specs {
            if let PackageSpec::S3 { s3, .. } = spec {
                s3.validate()?;
            }
        }
        let locals = specs.clone();
        let mut sources = tokio::task::spawn_blocking(move || Self::from_config(&locals, release))
            .await
            .map_err(|e| format!("Package source preparation failed: {e}"))??;
        for (index, spec) in specs.into_iter().enumerate() {
            if let PackageSpec::S3 { id, version, s3 } = spec {
                let origin = s3.origin();
                let provider =
                    crate::storage_providers::store::resolve(providers, &s3.provider, &policy)
                        .await?;
                let archive = s3::download(&s3, &provider, &cache, &policy)
                    .await
                    .map_err(|e| format!("{origin}: {e}"))?;
                sources = tokio::task::spawn_blocking(move || {
                    let source = sources.acquire(&archive, id.as_deref(), version.as_deref(), Some(release))
                        .map_err(|e| format!("{origin}: {e}"))?;
                    tracing::info!(source = %origin, package = %source.name, version = %source.version, "Prepared pinned S3 package");
                    sources.configured.insert(index, source);
                    Ok::<_, String>(sources)
                }).await.map_err(|e| format!("S3 package preparation failed: {e}"))??;
            }
        }
        Ok(sources)
    }

    pub(crate) fn configured(&self, index: usize) -> Option<&LocalPackageSpec> {
        self.configured.get(&index)
    }

    pub(crate) fn local_packages(&self) -> Vec<LocalPackageSpec> {
        self.packages
            .values()
            .map(|(source, _)| source.clone())
            .collect()
    }

    /// Resolve exact versions iteratively before handing installation to the manager.
    /// Installed packages are authoritative; cached archives supply their own manifests.
    pub(crate) fn prepare_offline(
        &mut self,
        roots: &[LoadedPackage],
        installed: &HashSet<String>,
        cache: &Path,
        release: FhirVersion,
    ) -> Result<(), String> {
        let mut pending: Vec<_> = roots
            .iter()
            .filter_map(|root| {
                root.version.as_ref().map(|version| {
                    (
                        (root.id.clone(), version.clone()),
                        "configuration".to_string(),
                        0,
                    )
                })
            })
            .collect();
        let mut visited = BTreeSet::new();
        while let Some(((name, version), parent, depth)) = pending.pop() {
            if depth > 64 {
                return Err(format!(
                    "Offline dependency graph exceeds depth limit at {name}@{version}"
                ));
            }
            validate_identity(&name, &version)?;
            let key = (name.clone(), version.clone());
            if !visited.insert(key.clone()) || installed.contains(&format!("{name}@{version}")) {
                continue;
            }
            if visited.len() > MAX_PACKAGES {
                return Err("Offline dependency graph exceeds 4096 packages".into());
            }
            if !self.packages.contains_key(&key) {
                let cached = cache.join(format!("{name}-{version}.tgz"));
                if !cached.is_file() {
                    return Err(format!(
                        "Offline package source missing: {name}@{version} (required by {parent}). Supply a packages.load path or {}",
                        cached.display()
                    ));
                }
                // Dependencies may target another release, e.g. shared terminology.
                // Schema compilation still filters by the active release.
                self.acquire(
                    &cached,
                    Some(&name),
                    Some(&version),
                    (depth == 0).then_some(release),
                )?;
            }
            let (_, manifest) = &self.packages[&key];
            let mut dependencies: Vec<_> = manifest.dependencies.iter().collect();
            dependencies.sort();
            pending.extend(dependencies.into_iter().map(|(dep, version)| {
                (
                    (dep.clone(), version.clone()),
                    format!("{name}@{}", manifest.version),
                    depth + 1,
                )
            }));
        }
        Ok(())
    }

    fn acquire(
        &mut self,
        path: &Path,
        id: Option<&str>,
        version: Option<&str>,
        release: Option<FhirVersion>,
    ) -> Result<LocalPackageSpec, String> {
        let path = std::fs::canonicalize(path)
            .map_err(|e| format!("Cannot read local package {}: {e}", path.display()))?;
        let owned = path.is_file();
        let mut root = if owned {
            let workspace = archive::extract(&path)?;
            let root = workspace.path().to_owned();
            self.workspaces.push(workspace);
            root
        } else if path.is_dir() {
            path
        } else {
            return Err("Package source must be a directory or a .tgz archive".into());
        };
        if !root.join("package.json").is_file() && root.join("package/package.json").is_file() {
            root = root.join("package");
        }
        validate_directory(&root)?;
        let manifest_path = root.join("package.json");
        let file = std::fs::File::open(&manifest_path)
            .map_err(|e| format!("Local package {} needs package.json: {e}", root.display()))?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Local package manifest exceeds 1 MiB".into());
        }
        let mut raw: serde_json::Value = serde_json::from_reader(file).map_err(|e| {
            format!(
                "Invalid local package manifest {}: {e}",
                manifest_path.display()
            )
        })?;
        // Older published HL7 packages use the field named in the package
        // specification. Normalize only an owned working copy for canonical-manager.
        let legacy_versions = raw.get("fhir-version-list").cloned();
        if let (Some(current), Some(legacy)) = (raw.get("fhirVersions"), &legacy_versions)
            && current != legacy
        {
            return Err("Package manifest has conflicting FHIR version declarations".into());
        }
        let normalize = raw.get("fhirVersions").is_none() && legacy_versions.is_some();
        if normalize {
            raw["fhirVersions"] = legacy_versions.unwrap_or_default();
        }
        let manifest: PackageManifest = serde_json::from_value(raw.clone()).map_err(|e| {
            format!(
                "Invalid local package manifest {}: {e}",
                manifest_path.display()
            )
        })?;
        validate_identity(&manifest.name, &manifest.version)?;
        for (dependency, version) in &manifest.dependencies {
            validate_identity(dependency, version)?;
        }
        if id.is_some_and(|id| id != manifest.name)
            || version.is_some_and(|v| v != manifest.version)
        {
            return Err(format!(
                "Local package identity does not match manifest {}@{}",
                manifest.name, manifest.version
            ));
        }
        if let Some(release) = release
            && !manifest.fhir_versions.as_ref().is_some_and(|versions| {
                versions
                    .iter()
                    .any(|v| v.parse::<FhirVersion>().ok() == Some(release))
            })
        {
            return Err(format!(
                "Local package {}@{} does not declare FHIR {}",
                manifest.name,
                manifest.version,
                release.release_name()
            ));
        }
        if normalize {
            if !owned {
                let snapshot = snapshot_directory(&root)?;
                root = snapshot.path().to_owned();
                self.workspaces.push(snapshot);
            }
            std::fs::write(
                root.join("package.json"),
                serde_json::to_vec(&raw).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Cannot prepare package manifest: {e}"))?;
        }
        let source = LocalPackageSpec {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            path: root,
            priority: 1,
        };
        let key = (manifest.name.clone(), manifest.version.clone());
        if let Some((existing, _)) = self.packages.get(&key) {
            if existing.path != source.path {
                return Err(format!(
                    "Multiple local sources declare {}@{}",
                    source.name, source.version
                ));
            }
            return Ok(existing.clone());
        }
        self.packages.insert(key, (source.clone(), manifest));
        Ok(source)
    }
}

fn validate_identity(name: &str, version: &str) -> Result<(), String> {
    for value in [name, version] {
        if value.is_empty()
            || value.len() > 255
            || value == "."
            || value == ".."
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
        {
            return Err(format!(
                "Package identity must use safe, exact name/version components: {name}@{version}"
            ));
        }
    }
    Ok(())
}

fn safe_index_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !PathBuf::from(name).is_absolute()
}

fn validate_directory(root: &Path) -> Result<(), String> {
    let mut pending = vec![(root.to_owned(), 0)];
    let mut files = 0;
    let mut total_bytes = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 64 {
            return Err("Local package exceeds directory depth limit".into());
        }
        if std::fs::symlink_metadata(&directory)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Local package cannot contain symbolic links".into());
        }
        for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            files += 1;
            if files > MAX_ENTRIES {
                return Err("Local package has too many files".into());
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err("Local package cannot contain symbolic links".into());
            }
            if kind.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let bytes = entry.metadata().map_err(|e| e.to_string())?.len();
                total_bytes = total_bytes.saturating_add(bytes);
                if bytes > MAX_FILE_BYTES || total_bytes > MAX_TOTAL_BYTES {
                    return Err("Local package exceeds file or total size limits".into());
                }
            } else {
                return Err("Local package contains a non-regular file".into());
            }
        }
    }
    let index = root.join(".index.json");
    if index.is_file() {
        let index: PackageIndex =
            serde_json::from_reader(std::fs::File::open(index).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid local package index: {e}"))?;
        for (filename, entry) in index.files {
            if !safe_index_component(&filename)
                || entry
                    .resource_type
                    .as_ref()
                    .is_some_and(|r| !safe_index_component(r))
            {
                return Err("Local package index contains an unsafe path".into());
            }
            let paths = [
                root.join(&filename),
                root.join("package").join(&filename),
                root.join(entry.resource_type.as_deref().unwrap_or(""))
                    .join(&filename),
            ];
            if !paths.iter().any(|p| p.is_file()) {
                return Err(format!(
                    "Local package index references missing file: {filename}"
                ));
            }
        }
    }
    Ok(())
}

/// Copy an already validated, bounded directory without inheriting read-only modes.
fn snapshot_directory(source: &Path) -> Result<tempfile::TempDir, String> {
    let snapshot = tempfile::Builder::new()
        .prefix("octofhir-package-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let mut pending = vec![(source.to_owned(), snapshot.path().to_owned())];
    while let Some((input, output)) = pending.pop() {
        for entry in std::fs::read_dir(input).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let target = output.join(entry.file_name());
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                std::fs::create_dir(&target).map_err(|e| e.to_string())?;
                pending.push((entry.path(), target));
            } else if kind.is_file() {
                let mut input = std::fs::File::open(entry.path()).map_err(|e| e.to_string())?;
                let mut output = std::fs::File::create(target).map_err(|e| e.to_string())?;
                std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            } else {
                return Err("Package directory changed during acquisition".into());
            }
        }
    }
    Ok(snapshot)
}
