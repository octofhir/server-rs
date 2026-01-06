use std::sync::{Arc, OnceLock, RwLock};

use crate::config::{AppConfig, PackageSpec};
use arc_swap::ArcSwap;
use octofhir_core::fhir::FhirVersion;
use octofhir_db_postgres::PostgresPackageStore;
use octofhir_fhirschema::types::StructureDefinition;
use octofhir_search::loader::parse_search_parameter;
use octofhir_search::registry::SearchParameterRegistry;
use std::str::FromStr;

/// Information about a loaded canonical package.
#[derive(Debug, Clone)]
pub struct LoadedPackage {
    pub id: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
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

/// Lock-free manager access using ArcSwap for high-performance reads.
/// This avoids RwLock contention in the hot path of terminology lookups.
static MANAGER: OnceLock<ArcSwap<Option<Arc<octofhir_canonical_manager::CanonicalManager>>>> =
    OnceLock::new();

pub fn set_registry(reg: Arc<RwLock<CanonicalRegistry>>) {
    // Initialize the lock-free manager cache from the registry
    if let Ok(guard) = reg.read() {
        let manager_opt = guard.manager.clone();
        let _ = MANAGER.get_or_init(|| ArcSwap::from_pointee(manager_opt));
    }
    let _ = REGISTRY.set(reg);
}

/// Update the lock-free manager cache. Called when rebuilding the registry.
fn update_manager_cache(manager: Option<Arc<octofhir_canonical_manager::CanonicalManager>>) {
    if let Some(swap) = MANAGER.get() {
        swap.store(Arc::new(manager));
    } else {
        let _ = MANAGER.get_or_init(|| ArcSwap::from_pointee(manager));
    }
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
    // Update lock-free manager cache first
    update_manager_cache(new_val.manager.clone());
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
///
/// Uses lock-free ArcSwap for high-performance reads without RwLock contention.
/// This is critical for terminology lookups which happen on every validation.
pub fn get_manager() -> Option<Arc<octofhir_canonical_manager::CanonicalManager>> {
    // Fast path: use lock-free ArcSwap
    if let Some(swap) = MANAGER.get() {
        let guard = swap.load();
        if let Some(ref manager) = **guard {
            return Some(Arc::clone(manager));
        }
    }
    // Fallback: check registry (for initialization edge cases)
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

    tracing::info!(
        base_dir = %base_dir,
        packages_dir = %fcm_cfg.storage.packages_dir.display(),
        cache_dir = %fcm_cfg.storage.cache_dir.display(),
        "canonical manager directory configuration"
    );

    // Ensure required directories exist (including index dir mentioned in config comments)
    let index_dir = base.join("index");
    for dir in [
        &base,
        &fcm_cfg.storage.packages_dir,
        &fcm_cfg.storage.cache_dir,
        &index_dir,
    ] {
        match tokio::fs::create_dir_all(dir).await {
            Ok(()) => {
                tracing::debug!("created/verified directory: {:?}", dir);
            }
            Err(e) => {
                tracing::error!(
                    "FAILED to create directory {:?}: {} (this may cause package installation to fail)",
                    dir,
                    e
                );
            }
        }
    }

    // Collect installable specs (require id and version)
    let mut install_specs: Vec<LoadedPackage> = Vec::new();
    for item in &cfg.packages.load {
        match normalize_spec(item) {
            Ok(pkg) => {
                if let (id, Some(ver)) = (pkg.id.clone(), pkg.version.clone()) {
                    fcm_cfg.add_package(&id, &ver, Some(1));
                    install_specs.push(pkg);
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
        install_specs.push(LoadedPackage {
            id: core_id,
            version: Some(core_ver),
            path: None,
            url: None,
        });
    }

    // Add SQL on FHIR package when feature is enabled
    if cfg.sql_on_fhir.enabled {
        let sof_id = "hl7.fhir.uv.sql-on-fhir".to_string();
        let sof_ver = "2.0.0-ballot".to_string();
        let sof_url = Some("https://build.fhir.org/ig/FHIR/sql-on-fhir-v2/package.tgz".to_string());

        // Check if not already in the list
        let already_added = install_specs
            .iter()
            .any(|pkg| pkg.id == sof_id);
        if !already_added {
            tracing::info!(
                "SQL on FHIR enabled; adding package {}@{} from URL",
                sof_id,
                sof_ver
            );
            // Note: We don't call fcm_cfg.add_package for URL-based packages
            // since they're not in the registry
            install_specs.push(LoadedPackage {
                id: sof_id,
                version: Some(sof_ver),
                path: None,
                url: sof_url,
            });
        }
    }

    // Auto-load HL7 terminology package for the FHIR version
    // This provides local ValueSets/CodeSystems and prevents remote calls to tx.fhir.org
    {
        let (term_id, term_ver) = default_terminology_for(desired);

        // Check if not already in the list
        let already_added = install_specs
            .iter()
            .any(|pkg| pkg.id == term_id);
        if !already_added {
            tracing::info!(
                "Auto-loading terminology package {}@{} for FHIR {}",
                term_id,
                term_ver,
                display_fhir(desired)
            );
            fcm_cfg.add_package(&term_id, &term_ver, Some(1));
            install_specs.push(LoadedPackage {
                id: term_id,
                version: Some(term_ver),
                path: None,
                url: None,
            });
        }
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
    // Skip preflight for URL-based packages as they don't come from registry
    let packages_to_validate: Vec<(String, String)> = install_specs
        .iter()
        .filter(|pkg| {
            if pkg.url.is_some() {
                // Skip validation for URL-based packages
                return false;
            }
            if let Some(ref version) = pkg.version {
                let key = format!("{}@{}", pkg.id, version);
                if installed_packages.contains(&key) {
                    tracing::info!(package = %key, "package already installed, skipping preflight");
                    false
                } else {
                    true
                }
            } else {
                false
            }
        })
        .map(|pkg| (pkg.id.clone(), pkg.version.clone().unwrap()))
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
            packages = %install_specs.iter().filter_map(|pkg| pkg.version.as_ref().map(|v| format!("{}@{}", pkg.id, v))).collect::<Vec<_>>().join(", "),
            count = install_specs.len(),
            "all packages already installed, skipping preflight"
        );
    }

    // Create manager with PostgreSQL storage
    let manager: std::sync::Arc<CanonicalManager> = match CanonicalManager::new_with_components(
        fcm_cfg,
        postgres_store.clone(),
        Arc::new(registry_client),
        postgres_store.clone(),
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
    let fhir_version_str = &cfg.fhir.version;

    // Collect packages to install (filter out already installed)
    let packages_to_install: Vec<&LoadedPackage> = install_specs
        .iter()
        .filter(|pkg| {
            if let Some(ref version) = pkg.version {
                let key = format!("{}@{}", pkg.id, version);
                if installed_packages.contains(&key) {
                    tracing::debug!(package = %key, "package already installed, skipping install");
                    false
                } else {
                    true
                }
            } else {
                false
            }
        })
        .collect();

    // Install packages - manager handles dependencies internally
    // Use batch installation with PackageSpec to support URL-based packages
    let mut successfully_installed: Vec<(String, String)> = Vec::new();
    for pkg in &packages_to_install {
        let name = &pkg.id;
        let version = pkg.version.as_ref().unwrap();

        tracing::info!(
            package = %name,
            version = %version,
            url = ?pkg.url,
            "attempting to install package via canonical manager"
        );

        // Create PackageSpec with URL if available
        let spec = octofhir_canonical_manager::PackageSpec {
            name: name.clone(),
            version: version.clone(),
            priority: 1,
            url: pkg.url.clone(),
        };

        // Use install_packages_batch for consistency
        match manager.install_packages_batch(vec![spec]).await {
            Ok(()) => {
                tracing::info!(
                    package = %name,
                    version = %version,
                    "successfully installed package"
                );
                successfully_installed.push((name.clone(), version.clone()));
            }
            Err(e) => {
                tracing::error!(
                    package = %name,
                    version = %version,
                    error = %e,
                    error_debug = ?e,
                    "failed to install package"
                );
            }
        }
    }

    // Convert schemas once for all packages at the end (more efficient than per-package)
    tracing::info!(
        count = install_specs.len(),
        "converting schemas for all packages"
    );
    for pkg in &install_specs {
        if let Some(ref version) = pkg.version
            && let Err(e) = convert_and_store_package_schemas(
                postgres_store.as_ref(),
                &pkg.id,
                version,
                fhir_version_str,
            )
            .await
            {
                tracing::warn!(
                    package = %pkg.id,
                    version = %version,
                    error = %e,
                    "failed to convert schemas for package"
                );
            }
    }

    // Load embedded packages: octofhir-auth and octofhir-app
    // These packages are split for better organization: auth resources vs app resources
    let fhir_version = &cfg.fhir.version;

    // Helper function to load an embedded package
    async fn load_embedded_package(
        postgres_store: &PostgresPackageStore,
        package_name: &str,
        package_version: &str,
        fhir_version: &str,
        resources_list: &[(&str, &str)],
    ) -> bool {
        tracing::info!(
            package = package_name,
            version = package_version,
            fhir_version = fhir_version,
            "Loading embedded package"
        );

        let embedded_resources: Result<Vec<(String, serde_json::Value)>, String> = resources_list
            .iter()
            .map(|(filename, content)| {
                let resource: serde_json::Value = serde_json::from_str(content)
                    .map_err(|e| format!("Failed to parse {}: {}", filename, e))?;
                let resource_type = resource["resourceType"]
                    .as_str()
                    .ok_or_else(|| format!("Missing resourceType in {}", filename))?
                    .to_string();
                Ok((resource_type, resource))
            })
            .collect();

        match embedded_resources {
            Ok(resources) => {
                let loaded_ok = {
                    match postgres_store
                        .load_from_embedded(package_name, package_version, fhir_version, resources)
                        .await
                    {
                        Ok(()) => {
                            tracing::info!(
                                package = package_name,
                                version = package_version,
                                fhir_version = fhir_version,
                                "Successfully loaded embedded package"
                            );
                            true
                        }
                        Err(e) => {
                            tracing::error!(
                                package = package_name,
                                version = package_version,
                                error = %e,
                                "Failed to load embedded package"
                            );
                            false
                        }
                    }
                };

                // Convert and store FHIRSchemas
                if loaded_ok
                    && let Err(e) = convert_and_store_package_schemas(
                        postgres_store,
                        package_name,
                        package_version,
                        fhir_version,
                    )
                    .await
                    {
                        tracing::warn!(
                            package = package_name,
                            version = package_version,
                            error = %e,
                            "Failed to convert schemas for embedded package"
                        );
                    }

                loaded_ok
            }
            Err(e) => {
                tracing::error!(
                    package = package_name,
                    error = %e,
                    "Failed to parse embedded resources"
                );
                false
            }
        }
    }

    // Load octofhir-auth package
    load_embedded_package(
        &postgres_store,
        "octofhir-auth",
        "0.1.0",
        fhir_version,
        crate::bootstrap::EMBEDDED_AUTH_RESOURCES,
    )
    .await;

    // Load octofhir-app package
    load_embedded_package(
        &postgres_store,
        "octofhir-app",
        "0.1.0",
        fhir_version,
        crate::bootstrap::EMBEDDED_APP_RESOURCES,
    )
    .await;

    // Load octofhir-notifications package
    load_embedded_package(
        &postgres_store,
        "octofhir-notifications",
        "0.1.0",
        fhir_version,
        crate::bootstrap::EMBEDDED_NOTIFICATIONS_RESOURCES,
    )
    .await;

    // Load CanonicalResource for R4 when SQL on FHIR is enabled
    // CanonicalResource is an R4B/R5 abstract type that ViewDefinition depends on
    if cfg.sql_on_fhir.enabled && fhir_version == "R4" {
        load_embedded_package(
            &postgres_store,
            "octofhir-sof-compat",
            "0.1.0",
            fhir_version,
            crate::bootstrap::EMBEDDED_SOF_COMPAT_RESOURCES,
        )
        .await;
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
                    url: None,
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
        .test_before_acquire(false)
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

/// Convert StructureDefinitions from a package to FhirSchemas and store them in the database.
///
/// This is called after a package is installed to pre-compute FhirSchemas from
/// StructureDefinitions. The model provider then loads these on-demand.
pub async fn convert_and_store_package_schemas(
    postgres_store: &PostgresPackageStore,
    package_name: &str,
    package_version: &str,
    fhir_version: &str,
) -> Result<usize, String> {
    tracing::info!(
        package = %package_name,
        version = %package_version,
        "converting StructureDefinitions to FhirSchemas"
    );

    // Query StructureDefinitions from the package
    let sds: Vec<StructureDefinition> = match postgres_store
        .find_resources_by_package_and_type(package_name, package_version, "StructureDefinition")
        .await
    {
        Ok(resources) => resources
            .into_iter()
            .filter_map(|resource| serde_json::from_value::<StructureDefinition>(resource).ok())
            .collect(),
        Err(e) => {
            tracing::warn!(
                package = %package_name,
                version = %package_version,
                error = %e,
                "failed to query StructureDefinitions"
            );
            return Err(format!("failed to query StructureDefinitions: {}", e));
        }
    };

    if sds.is_empty() {
        tracing::debug!(
            package = %package_name,
            version = %package_version,
            "no StructureDefinitions found in package"
        );
        return Ok(0);
    }

    tracing::info!(
        package = %package_name,
        version = %package_version,
        count = sds.len(),
        "converting StructureDefinitions to FhirSchemas"
    );

    // Convert StructureDefinitions to FhirSchemas
    // Store as tuples matching the batch function signature
    let mut schemas_to_store: Vec<(
        String,            // url
        Option<String>,    // version
        String,            // package_name
        String,            // package_version
        String,            // fhir_version
        String,            // schema_type
        serde_json::Value, // content
    )> = Vec::new();
    let mut success_count = 0;
    let mut error_count = 0;

    for sd in &sds {
        match octofhir_fhirschema::translate(sd.clone(), None) {
            Ok(schema) => {
                // Determine schema_type from SD metadata
                let schema_type = determine_schema_type(sd);
                let schema_json = match serde_json::to_value(&schema) {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::warn!(
                            sd_url = %sd.url,
                            error = %e,
                            "failed to serialize FhirSchema"
                        );
                        error_count += 1;
                        continue;
                    }
                };

                schemas_to_store.push((
                    sd.url.clone(),
                    sd.version.clone(),
                    package_name.to_string(),
                    package_version.to_string(),
                    fhir_version.to_string(),
                    schema_type,
                    schema_json,
                ));
                success_count += 1;
            }
            Err(e) => {
                tracing::trace!(
                    sd_url = %sd.url,
                    error = %e,
                    "failed to convert StructureDefinition to FhirSchema"
                );
                error_count += 1;
            }
        }
    }

    // Batch store all schemas
    if !schemas_to_store.is_empty() {
        // Convert to references for the batch function
        let refs: Vec<_> = schemas_to_store
            .iter()
            .map(
                |(url, version, pkg_name, pkg_version, fhir_ver, schema_type, content)| {
                    (
                        url.as_str(),
                        version.as_deref(),
                        pkg_name.as_str(),
                        pkg_version.as_str(),
                        fhir_ver.as_str(),
                        schema_type.as_str(),
                        content,
                    )
                },
            )
            .collect();

        match postgres_store.store_fhirschemas_batch(&refs).await {
            Ok(stored) => {
                tracing::info!(
                    package = %package_name,
                    version = %package_version,
                    stored = stored,
                    converted = success_count,
                    errors = error_count,
                    "stored FhirSchemas in database"
                );
            }
            Err(e) => {
                tracing::error!(
                    package = %package_name,
                    version = %package_version,
                    error = %e,
                    "failed to store FhirSchemas"
                );
                return Err(format!("failed to store FhirSchemas: {}", e));
            }
        }
    }

    Ok(success_count)
}

/// Determine the schema_type from StructureDefinition metadata.
fn determine_schema_type(sd: &StructureDefinition) -> String {
    let kind = sd.kind.as_str();
    let derivation = sd.derivation.as_deref();

    match (kind, derivation) {
        ("resource", Some("specialization")) => "resource".to_string(),
        ("resource", Some("constraint")) => "profile".to_string(),
        ("complex-type", _) => "complex-type".to_string(),
        ("primitive-type", _) => "primitive-type".to_string(),
        ("logical", _) => "logical".to_string(),
        _ => {
            // Check if it's an Extension
            let type_name = sd.type_name.as_str();
            if type_name == "Extension"
                || sd
                    .base_definition
                    .as_ref()
                    .is_some_and(|b| b.contains("Extension"))
            {
                "extension".to_string()
            } else {
                "resource".to_string() // Default
            }
        }
    }
}

fn normalize_spec(spec: &PackageSpec) -> Result<LoadedPackage, String> {
    match spec {
        PackageSpec::Simple(s) => parse_simple_spec(s),
        PackageSpec::Table { id, version, path, url } => {
            if id.as_deref().unwrap_or("").is_empty() && path.as_deref().unwrap_or("").is_empty() && url.is_none() {
                return Err("package table requires either 'id', 'path', or 'url'".into());
            }
            Ok(LoadedPackage {
                id: id.clone().unwrap_or_default(),
                version: version.clone(),
                path: path.clone(),
                url: url.clone(),
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
        url: None,
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

/// Returns the default HL7 terminology package for a given FHIR version.
/// These packages contain standard ValueSets and CodeSystems used by FHIR core.
/// Loading them locally prevents remote calls to tx.fhir.org for common terminology.
fn default_terminology_for(v: FhirVersion) -> (String, String) {
    match v {
        FhirVersion::R4 => ("hl7.terminology.r4".to_string(), "7.0.1".to_string()),
        FhirVersion::R4B => ("hl7.terminology.r4b".to_string(), "6.0.2".to_string()),
        FhirVersion::R5 => ("hl7.terminology.r5".to_string(), "7.0.1".to_string()),
        // R6 doesn't have its own terminology package yet, use R5
        FhirVersion::R6 => ("hl7.terminology.r5".to_string(), "7.0.1".to_string()),
    }
}

// ============================================================================
// Runtime Package Installation
// ============================================================================

/// Result of a runtime package installation.
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Package name that was installed
    pub name: String,
    /// Package version that was installed
    pub version: String,
    /// FHIR version of the package
    pub fhir_version: String,
    /// Number of resources in the package
    pub resource_count: usize,
    /// Whether search parameters were reloaded
    pub search_registry_rebuilt: bool,
}

/// Information about an installed package (used for dependencies).
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledPackageInfo {
    pub name: String,
    pub version: String,
    pub fhir_version: String,
    pub resource_count: usize,
}

/// Result of a parallel package installation with dependencies.
#[derive(Debug, Clone)]
pub struct ParallelInstallResult {
    /// Main package name that was requested
    pub name: String,
    /// Main package version that was requested
    pub version: String,
    /// FHIR version of the main package
    pub fhir_version: String,
    /// Number of resources in the main package
    pub resource_count: usize,
    /// Dependencies that were also installed
    pub dependencies_installed: Vec<InstalledPackageInfo>,
}

/// Install a package at runtime without requiring a server restart.
///
/// This function:
/// 1. Downloads and installs the package via canonical manager
/// 2. Rebuilds the canonical index
/// 3. Updates the global canonical registry
///
/// Note: FHIR version validation happens during installation by the canonical manager.
///
/// # Arguments
/// * `name` - Package name (e.g., "hl7.fhir.us.core")
/// * `version` - Package version (e.g., "6.1.0")
/// * `_server_fhir_version` - The server's configured FHIR version (for future validation)
///
/// # Errors
/// Returns an error if:
/// - The canonical manager is not available
/// - The package doesn't exist in the registry
/// - Package installation fails
pub async fn install_package_runtime(
    name: &str,
    version: &str,
    _server_fhir_version: &str,
) -> Result<InstallResult, String> {
    let manager = get_manager().ok_or_else(|| "canonical manager not available".to_string())?;

    tracing::info!(
        package = %name,
        version = %version,
        "installing package at runtime"
    );

    // Install the package (manager handles validation and dependencies)
    manager
        .install_package(name, version)
        .await
        .map_err(|e| format!("failed to install package {}@{}: {e}", name, version))?;

    // Get package info from storage to retrieve fhir_version and resource_count
    let packages = manager
        .storage()
        .list_packages()
        .await
        .map_err(|e| format!("failed to list packages after install: {e}"))?;

    let pkg_info = packages
        .iter()
        .find(|p| p.name == name && p.version == version)
        .ok_or_else(|| format!("package {}@{} not found after installation", name, version))?;

    let fhir_version = pkg_info.fhir_version.clone();
    let resource_count = pkg_info.resource_count;

    // Note: Old FHIRSchemas are deleted automatically in add_package (PostgresPackageStore)
    // and will be regenerated on-demand when needed

    // Update global registry
    let search_registry_rebuilt = update_global_registry_with_package(name, version).await;

    tracing::info!(
        package = %name,
        version = %version,
        fhir_version = %fhir_version,
        resource_count = resource_count,
        search_rebuilt = search_registry_rebuilt,
        "package installed successfully at runtime"
    );

    Ok(InstallResult {
        name: name.to_string(),
        version: version.to_string(),
        fhir_version,
        resource_count,
        search_registry_rebuilt,
    })
}

/// Install a package at runtime with full parallel download/extraction and background indexing.
///
/// This function uses a 4-stage parallel pipeline (handled by canonical-manager):
/// 1. Resolve all dependencies for the package
/// 2. Download ALL packages (main + deps) in parallel (8 concurrent)
/// 3. Extract ALL packages in parallel (using CPU cores)
/// 4. Batch index all packages
/// 5. Spawn background task for search index rebuild (non-blocking)
///
/// # Arguments
/// * `name` - Package name (e.g., "hl7.fhir.us.core")
/// * `version` - Package version (e.g., "6.1.0")
/// * `server_fhir_version` - The server's configured FHIR version
///
/// # Returns
/// Result containing main package info and list of installed dependencies
pub async fn install_package_parallel_runtime(
    name: &str,
    version: &str,
    _server_fhir_version: &str,
) -> Result<ParallelInstallResult, String> {
    use octofhir_canonical_manager::PackageSpec;

    let manager = get_manager().ok_or_else(|| "canonical manager not available".to_string())?;

    // Get packages before installation to detect what's new
    let packages_before: std::collections::HashSet<String> = manager
        .storage()
        .list_packages()
        .await
        .map_err(|e| format!("failed to list packages: {e}"))?
        .iter()
        .map(|p| format!("{}@{}", p.name, p.version))
        .collect();

    tracing::info!(
        package = %name,
        version = %version,
        "starting parallel installation with dependency resolution"
    );

    // Install with full parallel pipeline (resolves deps internally)
    let spec = PackageSpec {
        name: name.to_string(),
        version: version.to_string(),
        priority: 1,
        url: None,
    };

    manager
        .install_packages_parallel(vec![spec])
        .await
        .map_err(|e| format!("failed parallel installation of {}@{}: {e}", name, version))?;

    // Get packages after installation to find what was installed
    let packages_after = manager
        .storage()
        .list_packages()
        .await
        .map_err(|e| format!("failed to list packages after install: {e}"))?;

    let main_pkg = packages_after
        .iter()
        .find(|p| p.name == name && p.version == version)
        .ok_or_else(|| format!("main package {}@{} not found after install", name, version))?;

    // Find newly installed dependencies
    let dependencies_installed: Vec<InstalledPackageInfo> = packages_after
        .iter()
        .filter(|p| {
            let key = format!("{}@{}", p.name, p.version);
            !packages_before.contains(&key) && !(p.name == name && p.version == version)
        })
        .map(|p| InstalledPackageInfo {
            name: p.name.clone(),
            version: p.version.clone(),
            fhir_version: p.fhir_version.clone(),
            resource_count: p.resource_count,
        })
        .collect();

    let newly_installed_count = dependencies_installed.len() + 1; // +1 for main package

    // Spawn background task to update global registry
    let main_name = name.to_string();
    let main_version = version.to_string();
    let deps_for_registry: Vec<(String, String)> = dependencies_installed
        .iter()
        .map(|d| (d.name.clone(), d.version.clone()))
        .collect();

    tokio::spawn(async move {
        tracing::info!(
            package = %main_name,
            newly_installed = newly_installed_count,
            "updating global registry in background"
        );

        // Note: Old FHIRSchemas are deleted automatically in add_package (PostgresPackageStore)
        // and will be regenerated on-demand when needed

        // Update global registry
        update_global_registry_with_package(&main_name, &main_version).await;
        for (dep_name, dep_version) in &deps_for_registry {
            update_global_registry_with_package(dep_name, dep_version).await;
        }

        tracing::info!(
            package = %main_name,
            "global registry update completed"
        );
    });

    tracing::info!(
        package = %name,
        version = %version,
        dependencies = dependencies_installed.len(),
        "parallel package installation completed"
    );

    Ok(ParallelInstallResult {
        name: name.to_string(),
        version: version.to_string(),
        fhir_version: main_pkg.fhir_version.clone(),
        resource_count: main_pkg.resource_count,
        dependencies_installed,
    })
}

/// Update the global canonical registry after a package installation.
/// Returns true if the registry was updated successfully.
async fn update_global_registry_with_package(name: &str, version: &str) -> bool {
    if let Some(global) = get_registry()
        && let Ok(mut guard) = global.write() {
            // Add the new package to the registry
            let already_exists = guard
                .packages
                .iter()
                .any(|p| p.id == name && p.version.as_deref() == Some(version));
            if !already_exists {
                guard.packages.push(LoadedPackage {
                    id: name.to_string(),
                    version: Some(version.to_string()),
                    path: None,
                    url: None,
                });
            }
            return true;
        }
    false
}

/// Install a package at runtime with progress streaming via a channel.
///
/// This function spawns a background task that installs the package and sends
/// progress events through a channel. The caller can consume these events to
/// stream progress to clients via SSE.
///
/// # Arguments
/// * `name` - Package name (e.g., "hl7.fhir.us.core")
/// * `version` - Package version (e.g., "6.1.0")
///
/// # Returns
/// A receiver that yields `InstallEvent` messages during installation.
pub async fn install_package_runtime_with_progress(
    name: &str,
    version: &str,
) -> Result<tokio::sync::mpsc::UnboundedReceiver<octofhir_canonical_manager::InstallEvent>, String>
{
    use octofhir_canonical_manager::ChannelCallback;

    let manager = get_manager().ok_or_else(|| "canonical manager not available".to_string())?;

    let (callback, receiver) = ChannelCallback::new();
    let callback = std::sync::Arc::new(callback);
    let name = name.to_string();
    let version = version.to_string();

    // Spawn background task to perform the installation
    tokio::spawn(async move {
        tracing::info!(
            package = %name,
            version = %version,
            "installing package at runtime with progress"
        );

        let result = manager
            .install_package_with_callback(&name, &version, callback.clone())
            .await;

        match &result {
            Ok(()) => {
                tracing::info!(
                    package = %name,
                    version = %version,
                    "package installed successfully with progress tracking"
                );

                // Note: Old FHIRSchemas are deleted automatically in add_package (PostgresPackageStore)
                // and will be regenerated on-demand when needed

                // Update global registry
                let _ = update_global_registry_with_package(&name, &version).await;
            }
            Err(e) => {
                tracing::error!(
                    package = %name,
                    version = %version,
                    error = %e,
                    "package installation failed"
                );
            }
        }
    });

    Ok(receiver)
}

/// Uninstall a package at runtime.
///
/// # Note
/// This is a placeholder for future implementation. Currently, packages
/// cannot be uninstalled without a server restart.
pub async fn uninstall_package_runtime(_name: &str, _version: &str) -> Result<(), String> {
    Err("package uninstallation is not yet supported at runtime".to_string())
}

/// Lookup available versions for a package from the FHIR registry.
///
/// # Arguments
/// * `name` - Package name (e.g., "hl7.fhir.us.core")
///
/// # Returns
/// * `Ok(Vec<String>)` - List of available versions sorted by semver (newest first)
/// * `Err(String)` - Error message if lookup fails
pub async fn lookup_package_versions(name: &str) -> Result<Vec<String>, String> {
    let manager = get_manager().ok_or_else(|| "canonical manager not available".to_string())?;

    tracing::info!(package = %name, "looking up available versions from registry");

    manager
        .list_registry_versions(name)
        .await
        .map_err(|e| format!("failed to lookup package {}: {}", name, e))
}

/// Search result for a package in the registry.
#[derive(Debug, Clone)]
pub struct RegistrySearchResult {
    /// Package name
    pub name: String,
    /// Available versions (sorted by semver, newest first)
    pub versions: Vec<String>,
    /// Package description
    pub description: Option<String>,
    /// Latest version
    pub latest_version: String,
}

/// Search for packages in the FHIR registry.
///
/// This uses the fs.get-ig.org `/-/v1/search` endpoint which supports
/// partial matching (ILIKE). Spaces in the query are treated as wildcards.
///
/// # Arguments
/// * `query` - Search query string (e.g., "us core", "hl7.fhir")
///
/// # Returns
/// * `Ok(Vec<RegistrySearchResult>)` - List of matching packages
/// * `Err(String)` - Error message if search fails
pub async fn search_registry_packages(query: &str) -> Result<Vec<RegistrySearchResult>, String> {
    let manager = get_manager().ok_or_else(|| "canonical manager not available".to_string())?;

    tracing::info!(query = %query, "searching registry for packages");

    let results = manager
        .search_registry(query)
        .await
        .map_err(|e| format!("failed to search registry: {}", e))?;

    Ok(results
        .into_iter()
        .map(|pkg| RegistrySearchResult {
            name: pkg.name,
            versions: pkg.versions,
            description: pkg.description,
            latest_version: pkg.latest_version,
        })
        .collect())
}

// ============================================================================
// Search Parameter Registry Building
// ============================================================================

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
    let registry = SearchParameterRegistry::new();

    // Query ALL SearchParameter resources from canonical manager with pagination
    // The canonical manager has a max limit of 1000, so we need to paginate
    const PAGE_SIZE: usize = 1000;
    let mut offset = 0;
    let mut loaded_count = 0;
    let mut skipped_count = 0;

    loop {
        let search_results = manager
            .search()
            .await
            .resource_type("SearchParameter")
            .limit(PAGE_SIZE)
            .offset(offset)
            .execute()
            .await
            .map_err(|e| {
                format!(
                    "failed to query SearchParameter resources at offset {}: {e}",
                    offset
                )
            })?;

        let page_count = search_results.resources.len();
        tracing::debug!(
            offset = offset,
            page_count = page_count,
            "fetched SearchParameter page"
        );

        // Process this page
        for resource_match in &search_results.resources {
            match parse_search_parameter(&resource_match.resource.content) {
                Ok(param) => {
                    registry.register(param);
                    loaded_count += 1;
                }
                Err(e) => {
                    skipped_count += 1;
                    let url = resource_match
                        .resource
                        .content
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    tracing::warn!(url = %url, error = %e, "failed to parse SearchParameter resource");
                }
            }
        }

        // If we got fewer results than the page size, we've reached the end
        if page_count < PAGE_SIZE {
            break;
        }

        offset += PAGE_SIZE;
    }

    if registry.is_empty() {
        return Err("no search parameters loaded from canonical manager".to_string());
    }

    tracing::info!(
        loaded = loaded_count,
        skipped = skipped_count,
        total_in_registry = registry.len(),
        "search parameter registry built from canonical manager with pagination"
    );

    Ok(registry)
}
