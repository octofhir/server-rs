use octofhir_auth::config::AuthConfig;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub fhir: FhirSettings,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub search: SearchSettings,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub otel: OtelConfig,
    #[serde(default)]
    pub packages: PackagesConfig,
    /// Authentication and authorization configuration
    #[serde(default)]
    pub auth: AuthConfig,
}

// Default derived via field defaults

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        // Server validations
        if self.server.port == 0 {
            return Err("server.port must be > 0".into());
        }
        if self.server.read_timeout_ms == 0 || self.server.write_timeout_ms == 0 {
            return Err("server timeouts must be > 0".into());
        }
        // Search validations
        if self.search.default_count == 0 {
            return Err("search.default_count must be > 0".into());
        }
        if self.search.max_count == 0 {
            return Err("search.max_count must be > 0".into());
        }
        if self.search.default_count > self.search.max_count {
            return Err("search.default_count must be <= search.max_count".into());
        }
        // Logging validation
        let lvl = self.logging.level.to_ascii_lowercase();
        let valid_levels = ["trace", "debug", "info", "warn", "error", "off"];
        if !valid_levels.contains(&lvl.as_str()) {
            return Err(format!("logging.level must be one of {valid_levels:?}"));
        }
        // OTEL validation
        if self.otel.enabled && self.otel.endpoint.as_deref().unwrap_or("").is_empty() {
            return Err("otel.enabled=true requires otel.endpoint".into());
        }
        // FHIR version validation
        let v = self.fhir.version.to_ascii_uppercase();
        let allowed = ["R4", "R4B", "R5", "R6", "4.0.1", "4.3.0", "5.0.0", "6.0.0"];
        if !allowed.contains(&v.as_str()) {
            return Err("fhir.version must be one of R4, R4B, R5, R6".into());
        }
        // Storage backend validation
        if matches!(self.storage.backend, StorageBackend::Postgres) {
            if self.storage.postgres.is_none() {
                return Err(
                    "storage.postgres config is required when backend is 'postgres'".into(),
                );
            }
            if let Some(ref pg) = self.storage.postgres {
                if pg.url.is_empty() {
                    return Err("storage.postgres.url must not be empty".into());
                }
                if pg.pool_size == 0 {
                    return Err("storage.postgres.pool_size must be > 0".into());
                }
            }
        }
        // Auth validation
        if self.auth.enabled {
            self.auth
                .validate()
                .map_err(|e| format!("auth config error: {e}"))?;
        }
        Ok(())
    }

    pub fn addr(&self) -> SocketAddr {
        use std::net::{IpAddr, Ipv4Addr};
        let host: IpAddr = self
            .server
            .host
            .parse()
            .unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        SocketAddr::from((host, self.server.port))
    }

    pub fn read_timeout(&self) -> Duration {
        Duration::from_millis(self.server.read_timeout_ms as u64)
    }
    pub fn write_timeout(&self) -> Duration {
        Duration::from_millis(self.server.write_timeout_ms as u64)
    }

    /// Returns the base URL for the server.
    /// If `base_url` is configured, returns that; otherwise computes from host:port.
    pub fn base_url(&self) -> String {
        self.server
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", self.server.host, self.server.port))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Base URL for the server, used in links and responses.
    /// If not set, defaults to http://{host}:{port}
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: u32,
    #[serde(default = "default_write_timeout_ms")]
    pub write_timeout_ms: u32,
    #[serde(default = "default_body_limit")]
    pub body_limit_bytes: usize,
}

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    8080
}
fn default_read_timeout_ms() -> u32 {
    15_000
}
fn default_write_timeout_ms() -> u32 {
    15_000
}
fn default_body_limit() -> usize {
    1024 * 1024
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            base_url: None,
            read_timeout_ms: default_read_timeout_ms(),
            write_timeout_ms: default_write_timeout_ms(),
            body_limit_bytes: default_body_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: StorageBackend,
    /// In-memory storage options
    #[serde(default)]
    pub memory_limit_bytes: Option<usize>,
    #[serde(default)]
    pub preallocate_items: Option<usize>,
    /// PostgreSQL storage options
    #[serde(default)]
    pub postgres: Option<PostgresStorageConfig>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::InMemoryPapaya,
            memory_limit_bytes: None,
            preallocate_items: None,
            postgres: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StorageBackend {
    #[default]
    InMemoryPapaya,
    Postgres,
}

/// PostgreSQL storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresStorageConfig {
    /// Connection URL: `postgres://user:pass@host:port/database`
    pub url: String,
    /// Connection pool size (maximum number of connections)
    #[serde(default = "default_postgres_pool_size")]
    pub pool_size: u32,
    /// Connection timeout in milliseconds
    #[serde(default = "default_postgres_connect_timeout")]
    pub connect_timeout_ms: u64,
    /// Idle timeout in milliseconds
    #[serde(default)]
    pub idle_timeout_ms: Option<u64>,
    /// Whether to run migrations on startup
    #[serde(default = "default_postgres_run_migrations")]
    pub run_migrations: bool,
}

fn default_postgres_pool_size() -> u32 {
    10
}
fn default_postgres_connect_timeout() -> u64 {
    5000
}
fn default_postgres_run_migrations() -> bool {
    true
}

impl Default for PostgresStorageConfig {
    fn default() -> Self {
        Self {
            url: "postgres://localhost/octofhir".into(),
            pool_size: default_postgres_pool_size(),
            connect_timeout_ms: default_postgres_connect_timeout(),
            idle_timeout_ms: Some(300_000), // 5 minutes
            run_migrations: default_postgres_run_migrations(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSettings {
    #[serde(default = "default_search_default")]
    pub default_count: usize,
    #[serde(default = "default_search_max")]
    pub max_count: usize,
}
fn default_search_default() -> usize {
    10
}
fn default_search_max() -> usize {
    100
}
impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            default_count: default_search_default(),
            max_count: default_search_max(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}
fn default_log_level() -> String {
    "info".into()
}
impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OtelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub sample_ratio: Option<f64>,
    /// Optional deployment environment label, e.g., "dev", "staging", "prod"
    #[serde(default)]
    pub environment: Option<String>,
}
// Default derived

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirSettings {
    #[serde(default = "default_fhir_version")]
    pub version: String,
}
fn default_fhir_version() -> String {
    "R4".into()
}
impl Default for FhirSettings {
    fn default() -> Self {
        Self {
            version: default_fhir_version(),
        }
    }
}

/// Canonical package configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackagesConfig {
    /// List of package specs to load. Shorthand examples:
    /// - "hl7.fhir.r4b.core#4.3.0"
    /// - "hl7.terminology#5.5.0"
    ///   Optionally supports absolute/relative paths via table form in TOML.
    ///   When specified as tables, fields: { id = "...", version = "...", path = "..." }
    #[serde(default)]
    pub load: Vec<PackageSpec>,
    /// Optional directory where loaded packages will be stored.
    /// If unset, canonical manager defaults are used (~/.fcm/packages).
    #[serde(default)]
    pub path: Option<String>,
}

// Default derived

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PackageSpec {
    /// Shorthand: "package_id#version" or just "package_id"
    Simple(String),
    /// Expanded form for clarity or filesystem path loading
    Table {
        id: Option<String>,
        version: Option<String>,
        path: Option<String>,
    },
}

pub mod loader {
    use super::AppConfig;
    use config::{Config, Environment, File};
    use std::path::{Path, PathBuf};

    pub fn load_config(path: Option<&str>) -> Result<AppConfig, String> {
        let mut builder = Config::builder();
        match path {
            Some(p) => {
                let pathbuf = PathBuf::from(p);
                if pathbuf.exists() {
                    builder = builder.add_source(File::from(pathbuf));
                }
            }
            None => {
                // Try default root-level file
                let default_path = PathBuf::from("octofhir.toml");
                if default_path.exists() {
                    builder = builder.add_source(File::from(default_path));
                }
            }
        }
        // Environment variable overrides, e.g., OCTOFHIR__SERVER__PORT=9090
        builder = builder.add_source(
            Environment::with_prefix("OCTOFHIR")
                .try_parsing(true)
                .separator("__"),
        );
        let cfg = builder
            .build()
            .map_err(|e| format!("config build error: {e}"))?;
        let merged: AppConfig = cfg
            .try_deserialize()
            .map_err(|e| format!("config deserialize error: {e}"))?;
        // Validate
        merged.validate()?;
        Ok(merged)
    }

    pub fn load_config_with_default_path<P: AsRef<Path>>(
        path: Option<P>,
    ) -> Result<AppConfig, String> {
        let p = path
            .as_ref()
            .map(|p| p.as_ref().to_string_lossy().to_string());
        load_config(p.as_deref())
    }
}

pub mod shared {
    use super::AppConfig;
    use std::sync::{Arc, OnceLock, RwLock};

    static SHARED: OnceLock<Arc<RwLock<AppConfig>>> = OnceLock::new();

    pub fn set_shared(cfg: Arc<RwLock<AppConfig>>) {
        if let Some(existing) = SHARED.get() {
            if let (Ok(mut dst), Ok(src)) = (existing.write(), cfg.read()) {
                *dst = src.clone();
            }
        } else {
            let _ = SHARED.set(cfg);
        }
    }

    pub fn get() -> Option<&'static Arc<RwLock<AppConfig>>> {
        SHARED.get()
    }

    pub fn with_config<R>(f: impl FnOnce(&AppConfig) -> R) -> Option<R> {
        get().and_then(|arc| arc.read().ok().map(|g| f(&g)))
    }
}
