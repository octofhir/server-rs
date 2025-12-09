use std::sync::{Arc, OnceLock, RwLock};

use crate::config::{AppConfig, PackageSpec};
use octofhir_canonical_manager::SearchParameterInfo;
use octofhir_core::fhir::FhirVersion;
use octofhir_db_postgres::PostgresPackageStore;
use octofhir_search::parameters::{SearchParameter, SearchParameterType};
use octofhir_search::registry::SearchParameterRegistry;
use std::str::FromStr;

/// Information about a loaded canonical package.
#[derive(Debug, Clone)]
pub struct LoadedPackage {
    pub id: String,
    pub version: Option<String>,
    pub path: Option<String>,
}

/// Minimal, internal registry abstraction. This intentionally hides the
/// external canonical manager so we can evolve integration without
/// touching the rest of the codebase.
#[derive(Debug, Default, Clone)]
pub struct CanonicalRegistry {
    packages: Vec<LoadedPackage>,
    manager: Option<std::sync::Arc<octofhir_canonical_manager::CanonicalManager>>,
}

impl CanonicalRegistry {
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
            manager: None,
        }
    }

    pub fn list(&self) -> &[LoadedPackage] {
        &self.packages
    }

    fn add(&mut self, pkg: LoadedPackage) {
        self.packages.push(pkg)
    }
}

static REGISTRY: OnceLock<Arc<RwLock<CanonicalRegistry>>> = OnceLock::new();

pub fn set_registry(reg: Arc<RwLock<CanonicalRegistry>>) {
    let _ = REGISTRY.set(reg);
}

pub fn get_registry() -> Option<&'static Arc<RwLock<CanonicalRegistry>>> {
    REGISTRY.get()
}

pub fn with_registry<R>(f: impl FnOnce(&CanonicalRegistry) -> R) -> Option<R> {
    get_registry().and_then(|arc| arc.read().ok().map(|g| f(&g)))
}

/// Initialize the canonical registry from configuration. This performs
/// best-effort loading: it logs failures but keeps successfully loaded packages.
///
/// In a future phase, this function will call into the real
/// `octofhir-canonical-manager` to load packages by id/version or path.
pub async fn init_from_config_async(
    cfg: &AppConfig,
) -> Result<Arc<RwLock<CanonicalRegistry>>, String> {
    let reg = build_registry_with_manager(cfg).await?;
    Ok(Arc::new(RwLock::new(reg)))
}

/// Rebuild the registry from configuration and atomically swap its contents.
/// If a global registry is not yet set, this initializes it.
pub async fn rebuild_from_config_async(cfg: &AppConfig) -> Result<(), String> {
    let new_val = build_registry_with_manager(cfg).await?;
    if let Some(global) = get_registry() {
        if let Ok(mut guard) = global.write() {
            *guard = new_val;
        }
    } else {
        set_registry(Arc::new(RwLock::new(new_val)));
    }
    Ok(())
}

fn build_registry(cfg: &AppConfig) -> CanonicalRegistry {
    let mut registry = CanonicalRegistry::new();
    for item in &cfg.packages.load {
        match normalize_spec(item) {
            Ok(pkg) => registry.add(pkg),
            Err(e) => tracing::error!(error.kind = "package-parse", message = %e),
        }
    }
    registry
}

/// Get a clone of the underlying canonical manager, if available.
pub fn get_manager() -> Option<std::sync::Arc<octofhir_canonical_manager::CanonicalManager>> {
    get_registry().and_then(|arc| arc.read().ok().and_then(|g| g.manager.clone()))
}

async fn build_registry_with_manager(cfg: &AppConfig) -> Result<CanonicalRegistry, String> {
    use octofhir_canonical_manager::traits::PackageStore;
    use octofhir_canonical_manager::{CanonicalManager, FcmConfig};

    // Start with default FCM config; allow env overrides and quick init flags
    let mut fcm_cfg = FcmConfig::default();
    fcm_cfg.apply_env_overrides();
    // Determine base directory for manager storage
    let base_dir = cfg
        .packages
        .path
        .clone()
        .unwrap_or_else(|| ".fhir".to_string());
    let base = std::path::PathBuf::from(&base_dir);
    fcm_cfg.storage.packages_dir = base.join("packages");
    fcm_cfg.storage.cache_dir = base.join("cache");

    // Collect installable specs (require id and version)
    let mut install_specs: Vec<(String, String)> = Vec::new();
    for item in &cfg.packages.load {
        match normalize_spec(item) {
            Ok(pkg) => {
                if let (id, Some(ver)) = (pkg.id.clone(), pkg.version.clone()) {
                    fcm_cfg.add_package(&id, &ver, Some(1));
                    install_specs.push((id, ver));
                } else {
                    tracing::warn!("skipping package without version for canonical manager");
                }
            }
            Err(e) => tracing::error!(error.kind = "package-parse", message = %e),
        }
    }

    // Resolve desired FHIR version from config
    let desired = parse_fhir_version(&cfg.fhir.version)
        .ok_or_else(|| format!("unsupported fhir.version: {}", cfg.fhir.version))?;
    tracing::info!(fhir.version = %display_fhir(desired), configured = %cfg.fhir.version, "FHIR version resolved");

    // If no packages configured, add the default FHIR core package for the chosen version
    if install_specs.is_empty() {
        let (core_id, core_ver) = default_core_for(desired);
        tracing::info!(
            "no packages configured; using default core {}@{}",
            core_id,
            core_ver
        );
        fcm_cfg.add_package(&core_id, &core_ver, Some(1));
        install_specs.push((core_id, core_ver));
    }

    // Initialize PostgreSQL storage FIRST to check for already-installed packages
    let pg_cfg = cfg
        .storage
        .postgres
        .as_ref()
        .ok_or_else(|| "storage.postgres configuration is required".to_string())?;

    // Create PostgreSQL pool for FCM (separate from main storage pool)
    let pg_pool = create_fcm_postgres_pool(pg_cfg)
        .await
        .map_err(|e| format!("failed to create FCM PostgreSQL pool: {e}"))?;

    // Create PostgresPackageStore (implements both PackageStore and SearchStorage)
    let postgres_store = Arc::new(PostgresPackageStore::new(pg_pool));

    // Check which packages are already installed in the database
    let installed_packages: std::collections::HashSet<String> =
        match postgres_store.list_packages().await {
            Ok(pkgs) => pkgs
                .iter()
                .map(|p| format!("{}@{}", p.name, p.version))
                .collect(),
            Err(e) => {
                tracing::warn!("failed to list installed packages: {}", e);
                std::collections::HashSet::new()
            }
        };

    // Filter out already-installed packages from preflight check
    let packages_to_validate: Vec<(String, String)> = install_specs
        .iter()
        .filter(|(name, version)| {
            let key = format!("{}@{}", name, version);
            if installed_packages.contains(&key) {
                tracing::info!(package = %key, "package already installed, skipping preflight");
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();

    // Only do network preflight for packages that aren't already installed
    let storage = fcm_cfg.get_expanded_storage_config();
    let registry_client = octofhir_canonical_manager::registry::RegistryClient::new(
        &fcm_cfg.registry,
        storage.cache_dir.clone(),
    )
    .await
    .map_err(|e| format!("registry init error: {e}"))?;

    if !packages_to_validate.is_empty() {
        preflight_validate_all(&registry_client, &packages_to_validate, desired).await?;
        tracing::info!(
            packages = %packages_to_validate.iter().map(|(n,v)| format!("{n}@{v}")).collect::<Vec<_>>().join(", "),
            count = packages_to_validate.len(),
            "FHIR package preflight passed"
        );
    } else {
        tracing::info!(
            packages = %install_specs.iter().map(|(n,v)| format!("{n}@{v}")).collect::<Vec<_>>().join(", "),
            count = install_specs.len(),
            "all packages already installed, skipping preflight"
        );
    }

    // Create manager with PostgreSQL storage
    let manager: std::sync::Arc<CanonicalManager> = match CanonicalManager::new_with_components(
        fcm_cfg,
        postgres_store.clone(),
        Arc::new(registry_client),
        postgres_store,
    )
    .await
    {
        Ok(m) => {
            tracing::info!("canonical manager initialized with PostgreSQL storage");
            std::sync::Arc::new(m)
        }
        Err(e) => {
            tracing::error!("failed to initialize canonical manager: {}", e);
            return Ok(build_registry(cfg));
        }
    };

    // Install configured packages (manager handles dependencies)
    // Only install packages that aren't already in the database
    for (name, version) in &install_specs {
        let key = format!("{}@{}", name, version);
        if installed_packages.contains(&key) {
            tracing::debug!(package = %key, "package already installed, skipping install");
            continue;
        }
        if let Err(e) = manager.install_package(name, version).await {
            tracing::error!("failed to install package {}@{}: {}", name, version, e);
        }
    }
    // Rebuild index so lists reflect installed packages
    if let Err(e) = manager.force_full_rebuild().await {
        tracing::warn!("failed to rebuild canonical index: {}", e);
    }

    // Populate our registry view from manager
    let mut reg = CanonicalRegistry::new();
    #[allow(unused_mut)]
    let mut pkgs_vec = Vec::new();
    match manager.storage().list_packages().await {
        Ok(pkgs) => {
            for p in pkgs {
                pkgs_vec.push(LoadedPackage {
                    id: p.name,
                    version: Some(p.version),
                    path: None,
                });
            }
        }
        Err(e) => tracing::warn!("failed to list packages: {}", e),
    }
    reg.packages = pkgs_vec;
    reg.manager = Some(manager);
    tracing::info!(packages_loaded = %reg.packages.len(), storage = "PostgreSQL", "canonical manager initialized with packages");
    Ok(reg)
}

/// Creates a PostgreSQL connection pool for FCM storage.
async fn create_fcm_postgres_pool(
    pg_cfg: &crate::config::PostgresStorageConfig,
) -> Result<sqlx_postgres::PgPool, String> {
    use sqlx_postgres::PgPoolOptions;
    use std::time::Duration;

    let pool = PgPoolOptions::new()
        .max_connections(pg_cfg.pool_size)
        .acquire_timeout(Duration::from_millis(pg_cfg.connect_timeout_ms))
        .idle_timeout(pg_cfg.idle_timeout_ms.map(Duration::from_millis))
        .connect(&pg_cfg.connection_url())
        .await
        .map_err(|e| format!("failed to connect to PostgreSQL for FCM: {e}"))?;

    // Run FCM migrations
    octofhir_db_postgres::migrations::run(&pool, &pg_cfg.connection_url())
        .await
        .map_err(|e| format!("failed to run FCM migrations: {e}"))?;

    Ok(pool)
}

fn normalize_spec(spec: &PackageSpec) -> Result<LoadedPackage, String> {
    match spec {
        PackageSpec::Simple(s) => parse_simple_spec(s),
        PackageSpec::Table { id, version, path } => {
            if id.as_deref().unwrap_or("").is_empty() && path.as_deref().unwrap_or("").is_empty() {
                return Err("package table requires either 'id' or 'path'".into());
            }
            Ok(LoadedPackage {
                id: id.clone().unwrap_or_default(),
                version: version.clone(),
                path: path.clone(),
            })
        }
    }
}

fn parse_simple_spec(s: &str) -> Result<LoadedPackage, String> {
    if s.trim().is_empty() {
        return Err("empty package spec".into());
    }
    // Support "pkg#ver" or just "pkg"
    let (id, version) = match s.split_once('#') {
        Some((id, ver)) => (id.trim().to_string(), Some(ver.trim().to_string())),
        None => (s.trim().to_string(), None),
    };
    Ok(LoadedPackage {
        id,
        version,
        path: None,
    })
}

async fn preflight_validate_all(
    client: &octofhir_canonical_manager::registry::RegistryClient,
    roots: &[(String, String)],
    desired: FhirVersion,
) -> Result<(), String> {
    use std::collections::{HashSet, VecDeque};
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, String)> = roots.iter().cloned().collect();

    while let Some((name, version)) = queue.pop_front() {
        let key = format!("{name}@{version}");
        if !visited.insert(key.clone()) {
            continue;
        }
        let meta = client
            .get_package_metadata(&name, &version)
            .await
            .map_err(|e| format!("failed to fetch package metadata for {key}: {e}"))?;
        let pkg_ver = map_metadata_version(&meta.fhir_version);
        if pkg_ver != desired {
            // Use improved error formatting for better user experience
            let error_msg = format_version_mismatch_error(
                &name,
                &version,
                display_fhir(pkg_ver),
                display_fhir(desired),
                roots,
            );
            return Err(error_msg);
        }
        for (dep_name, dep_version) in meta.dependencies {
            queue.push_back((dep_name, dep_version));
        }
    }
    Ok(())
}

fn parse_fhir_version(s: &str) -> Option<FhirVersion> {
    let up = s.trim();
    FhirVersion::from_str(up).ok().or_else(|| {
        // allow numeric forms
        if up.starts_with("4.0") {
            Some(FhirVersion::R4)
        } else if up.starts_with("4.3") {
            Some(FhirVersion::R4B)
        } else if up.starts_with("5.0") {
            Some(FhirVersion::R5)
        } else if up.starts_with("6.0") {
            Some(FhirVersion::R6)
        } else {
            None
        }
    })
}

fn map_metadata_version(s: &str) -> FhirVersion {
    // Map registry metadata version to enum by prefix
    if s.starts_with("4.0") {
        FhirVersion::R4
    } else if s.starts_with("4.3") {
        FhirVersion::R4B
    } else if s.starts_with("5.0") {
        FhirVersion::R5
    } else if s.starts_with("6.0") {
        FhirVersion::R6
    } else {
        FhirVersion::R4
    }
}

/// Get the FHIR registry URL for a package.
fn get_package_registry_url(package_id: &str) -> String {
    format!("https://registry.fhir.org/package/{}", package_id)
}

/// Format a user-friendly error message for FHIR version mismatches.
///
/// This function creates a detailed error message that:
/// - Clearly states the problem
/// - Shows which package is affected
/// - Provides actionable remediation steps
/// - Lists all configured packages for context
fn format_version_mismatch_error(
    package_id: &str,
    package_version: &str,
    package_fhir_version: &str,
    server_fhir_version: &str,
    all_packages: &[(String, String)],
) -> String {
    let registry_url = get_package_registry_url(package_id);
    let packages_list = all_packages
        .iter()
        .map(|(name, ver)| format!("  - {}@{}", name, ver))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"
========================================================================
FHIR VERSION MISMATCH DETECTED
========================================================================

Package: {}@{}
Package FHIR Version: {}
Server FHIR Version: {}

All packages must target the same FHIR version as the server.

To fix this issue:

Option 1: Change server configuration to use FHIR {}
  - Update your config file:
    [fhir]
    version = "{}"

Option 2: Use a {}-compatible version of {}
  - Check {} for available versions
  - Update your config file to use a compatible version

Currently configured packages:
{}

For more information about FHIR package versions, see:
https://confluence.hl7.org/display/FHIR/NPM+Package+Specification

========================================================================
"#,
        package_id,
        package_version,
        package_fhir_version,
        server_fhir_version,
        package_fhir_version,
        package_fhir_version,
        server_fhir_version,
        package_id,
        registry_url,
        packages_list
    )
}

fn display_fhir(v: FhirVersion) -> &'static str {
    match v {
        FhirVersion::R4 => "R4",
        FhirVersion::R4B => "R4B",
        FhirVersion::R5 => "R5",
        FhirVersion::R6 => "R6",
    }
}

fn default_core_for(v: FhirVersion) -> (String, String) {
    match v {
        FhirVersion::R4 => ("hl7.fhir.r4.core".to_string(), "4.0.1".to_string()),
        FhirVersion::R4B => ("hl7.fhir.r4b.core".to_string(), "4.3.0".to_string()),
        FhirVersion::R5 => ("hl7.fhir.r5.core".to_string(), "5.0.0".to_string()),
        FhirVersion::R6 => ("hl7.fhir.r6.core".to_string(), "6.0.0".to_string()),
    }
}

// ============================================================================
// Search Parameter Registry Building
// ============================================================================

/// Convert a SearchParameterInfo from canonical manager to SearchParameter for search engine.
fn convert_to_search_param(info: &SearchParameterInfo) -> Option<SearchParameter> {
    let param_type = SearchParameterType::parse(&info.type_field)?;
    let mut sp = SearchParameter::new(
        &info.code,
        info.url.as_deref().unwrap_or(""),
        param_type,
        info.base.clone(),
    );
    if let Some(expr) = &info.expression {
        sp = sp.with_expression(expr);
    }
    if let Some(desc) = &info.description {
        sp = sp.with_description(desc);
    }
    Some(sp)
}

/// Build a SearchParameterRegistry by loading ALL SearchParameter resources from the canonical manager.
///
/// This function queries all SearchParameter resources from the canonical manager and registers
/// them in the search registry. The registry can then be used for validating and executing
/// FHIR search queries.
///
/// # Errors
/// Returns an error if:
/// - The canonical manager is unavailable
/// - No search parameters could be loaded
pub async fn build_search_registry(
    manager: &octofhir_canonical_manager::CanonicalManager,
) -> Result<SearchParameterRegistry, String> {
    let mut registry = SearchParameterRegistry::new();

    // Query ALL SearchParameter resources from canonical manager
    // The search method allows filtering by resource type
    let search_results = manager
        .search()
        .await
        .resource_type("SearchParameter")
        .limit(10000) // Ensure we get all search parameters
        .execute()
        .await
        .map_err(|e| format!("failed to query SearchParameter resources: {e}"))?;

    let mut loaded_count = 0;
    let mut skipped_count = 0;

    for resource_match in &search_results.resources {
        match SearchParameterInfo::from_resource(&resource_match.resource) {
            Ok(info) => {
                if let Some(sp) = convert_to_search_param(&info) {
                    registry.register(sp);
                    loaded_count += 1;
                } else {
                    skipped_count += 1;
                    tracing::debug!(
                        code = %info.code,
                        type_field = %info.type_field,
                        "skipping SearchParameter with unknown type"
                    );
                }
            }
            Err(e) => {
                skipped_count += 1;
                tracing::debug!(error = %e, "failed to parse SearchParameter resource");
            }
        }
    }

    if registry.is_empty() {
        return Err("no search parameters loaded from canonical manager".to_string());
    }

    tracing::info!(
        loaded = loaded_count,
        skipped = skipped_count,
        total_in_registry = registry.len(),
        "search parameter registry built from canonical manager"
    );

    Ok(registry)
}
