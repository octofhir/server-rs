use octofhir_auth::config::AuthConfig;
use octofhir_search::TerminologyConfig;
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
    /// Terminology service configuration
    #[serde(default)]
    pub terminology: TerminologyConfig,
    /// Validation configuration
    #[serde(default)]
    pub validation: ValidationSettings,
    /// Redis configuration
    #[serde(default)]
    pub redis: RedisConfig,
    /// Cache configuration
    #[serde(default)]
    pub cache: CacheConfig,
    /// DB Console configuration (SQL execution, LSP)
    #[serde(default)]
    pub db_console: DbConsoleConfig,
    /// Bootstrap configuration (initial admin user, default data)
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
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
        // Storage validation - PostgreSQL is required
        if self.storage.postgres.is_none() {
            return Err("storage.postgres config is required".into());
        }
        if let Some(ref pg) = self.storage.postgres {
            // Validate that we have either a URL or valid host/database
            if pg.url.is_none() && pg.host.is_empty() {
                return Err("storage.postgres requires either 'url' or 'host' to be set".into());
            }
            if pg.url.is_none() && pg.database.is_empty() {
                return Err("storage.postgres.database must not be empty".into());
            }
            if pg.pool_size == 0 {
                return Err("storage.postgres.pool_size must be > 0".into());
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
    /// PostgreSQL storage options (required)
    #[serde(default)]
    pub postgres: Option<PostgresStorageConfig>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            postgres: Some(PostgresStorageConfig::default()),
        }
    }
}

/// PostgreSQL storage configuration
///
/// Supports two modes:
/// 1. URL mode: Set `url` to a full connection string like `postgres://user:pass@host:port/database`
/// 2. Separate options mode: Set `host`, `port`, `user`, `password`, `database` individually
///
/// If `url` is set, it takes precedence. Otherwise, a URL is constructed from the separate options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresStorageConfig {
    /// Full connection URL: `postgres://user:pass@host:port/database`
    /// If set, this takes precedence over individual options.
    #[serde(default)]
    pub url: Option<String>,

    /// PostgreSQL host (default: localhost)
    #[serde(default = "default_postgres_host")]
    pub host: String,

    /// PostgreSQL port (default: 5432)
    #[serde(default = "default_postgres_port")]
    pub port: u16,

    /// PostgreSQL user (default: postgres)
    #[serde(default = "default_postgres_user")]
    pub user: String,

    /// PostgreSQL password (default: empty)
    #[serde(default)]
    pub password: Option<String>,

    /// PostgreSQL database name (default: octofhir)
    #[serde(default = "default_postgres_database")]
    pub database: String,

    /// Connection pool size (maximum number of connections)
    #[serde(default = "default_postgres_pool_size")]
    pub pool_size: u32,

    /// Connection timeout in milliseconds
    #[serde(default = "default_postgres_connect_timeout")]
    pub connect_timeout_ms: u64,

    /// Idle timeout in milliseconds
    #[serde(default)]
    pub idle_timeout_ms: Option<u64>,
}

fn default_postgres_host() -> String {
    "localhost".into()
}
fn default_postgres_port() -> u16 {
    5432
}
fn default_postgres_user() -> String {
    "postgres".into()
}
fn default_postgres_database() -> String {
    "octofhir".into()
}
fn default_postgres_pool_size() -> u32 {
    10
}
fn default_postgres_connect_timeout() -> u64 {
    5000
}

impl PostgresStorageConfig {
    /// Returns the connection URL.
    /// If `url` is set, returns it directly.
    /// Otherwise, constructs URL from individual options.
    pub fn connection_url(&self) -> String {
        if let Some(ref url) = self.url {
            return url.clone();
        }

        // Construct URL from individual options
        let password_part = self
            .password
            .as_ref()
            .map(|p| format!(":{}", p))
            .unwrap_or_default();

        format!(
            "postgres://{}{}@{}:{}/{}",
            self.user, password_part, self.host, self.port, self.database
        )
    }
}

impl Default for PostgresStorageConfig {
    fn default() -> Self {
        Self {
            url: None,
            host: default_postgres_host(),
            port: default_postgres_port(),
            user: default_postgres_user(),
            password: None,
            database: default_postgres_database(),
            pool_size: default_postgres_pool_size(),
            connect_timeout_ms: default_postgres_connect_timeout(),
            idle_timeout_ms: Some(300_000), // 5 minutes
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
    /// Optional directory for package cache (downloads from registry).
    /// If unset, canonical manager defaults are used (~/.fcm/packages).
    /// Note: Package data is stored in PostgreSQL's 'fcm' schema.
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

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSettings {
    /// Allow clients to skip validation via X-Skip-Validation header
    /// Default: false (disabled for security)
    #[serde(default = "default_allow_skip_validation")]
    pub allow_skip_validation: bool,
}

fn default_allow_skip_validation() -> bool {
    false
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            allow_skip_validation: default_allow_skip_validation(),
        }
    }
}

/// Redis configuration for horizontal scaling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Enable Redis (gracefully degrades without it)
    /// Default: false (disabled for single-instance deployments)
    #[serde(default = "default_redis_enabled")]
    pub enabled: bool,

    /// Redis connection URL (e.g., "redis://localhost:6379")
    #[serde(default = "default_redis_url")]
    pub url: String,

    /// Connection pool size
    #[serde(default = "default_redis_pool_size")]
    pub pool_size: usize,

    /// Connection timeout in milliseconds
    #[serde(default = "default_redis_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_redis_enabled() -> bool {
    false
}

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

fn default_redis_pool_size() -> usize {
    10
}

fn default_redis_timeout_ms() -> u64 {
    5000
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            enabled: default_redis_enabled(),
            url: default_redis_url(),
            pool_size: default_redis_pool_size(),
            timeout_ms: default_redis_timeout_ms(),
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Terminology cache TTL in seconds
    #[serde(default = "default_terminology_ttl_secs")]
    pub terminology_ttl_secs: u64,

    /// Local (L1) cache max entries
    #[serde(default = "default_local_cache_max_entries")]
    pub local_cache_max_entries: usize,
}

fn default_terminology_ttl_secs() -> u64 {
    3600 // 1 hour
}

fn default_local_cache_max_entries() -> usize {
    10000
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            terminology_ttl_secs: default_terminology_ttl_secs(),
            local_cache_max_entries: default_local_cache_max_entries(),
        }
    }
}

/// DB Console configuration for SQL execution and LSP features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConsoleConfig {
    /// Enable DB console functionality
    /// Default: true
    #[serde(default = "default_db_console_enabled")]
    pub enabled: bool,

    /// SQL execution mode:
    /// - "readonly": SELECT queries only (default)
    /// - "readwrite": SELECT, INSERT, UPDATE, DELETE
    /// - "admin": All SQL including DDL (CREATE, DROP, ALTER, etc.)
    #[serde(default = "default_sql_mode")]
    pub sql_mode: SqlMode,

    /// Required role for DB console access
    /// If set, user must have this role to access the DB console
    /// If not set, any authenticated user can access
    #[serde(default)]
    pub required_role: Option<String>,

    /// Enable LSP (Language Server Protocol) features
    /// Provides autocomplete, hover info, diagnostics for SQL
    /// Default: true
    #[serde(default = "default_lsp_enabled")]
    pub lsp_enabled: bool,
}

fn default_db_console_enabled() -> bool {
    true
}

fn default_sql_mode() -> SqlMode {
    SqlMode::Readonly
}

fn default_lsp_enabled() -> bool {
    true
}

impl Default for DbConsoleConfig {
    fn default() -> Self {
        Self {
            enabled: default_db_console_enabled(),
            sql_mode: default_sql_mode(),
            required_role: None,
            lsp_enabled: default_lsp_enabled(),
        }
    }
}

/// SQL execution mode for DB console
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlMode {
    /// Only SELECT queries allowed
    Readonly,
    /// SELECT, INSERT, UPDATE, DELETE allowed
    Readwrite,
    /// All SQL including DDL (CREATE, DROP, ALTER, TRUNCATE, GRANT, REVOKE)
    Admin,
}

impl SqlMode {
    /// Check if a query is allowed in this mode
    pub fn is_query_allowed(&self, query: &str) -> Result<(), String> {
        let query_upper = query.trim().to_uppercase();

        match self {
            SqlMode::Readonly => {
                // Must start with SELECT or WITH
                if !query_upper.starts_with("SELECT") && !query_upper.starts_with("WITH") {
                    return Err("Only SELECT queries are allowed in readonly mode".to_string());
                }
                // Check for dangerous keywords that could be in subqueries
                let forbidden = [
                    "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT",
                    "REVOKE",
                ];
                for kw in forbidden {
                    if query_upper.contains(kw) {
                        return Err(format!("{} is not allowed in readonly mode", kw));
                    }
                }
                Ok(())
            }
            SqlMode::Readwrite => {
                // DDL operations not allowed
                let forbidden = ["DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT", "REVOKE"];
                for kw in forbidden {
                    if query_upper.contains(kw) {
                        return Err(format!("{} is not allowed in readwrite mode", kw));
                    }
                }
                Ok(())
            }
            SqlMode::Admin => {
                // All queries allowed
                Ok(())
            }
        }
    }
}

impl std::fmt::Display for SqlMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlMode::Readonly => write!(f, "readonly"),
            SqlMode::Readwrite => write!(f, "readwrite"),
            SqlMode::Admin => write!(f, "admin"),
        }
    }
}

/// Bootstrap configuration for initial server setup
///
/// Configures admin user creation on first startup.
/// Admin credentials can also be set via environment variables:
/// - OCTOFHIR__BOOTSTRAP__ADMIN_USER__USERNAME
/// - OCTOFHIR__BOOTSTRAP__ADMIN_USER__PASSWORD
/// - OCTOFHIR__BOOTSTRAP__ADMIN_USER__EMAIL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Admin user configuration
    /// If set, creates an admin user on first startup (if not already exists)
    #[serde(default)]
    pub admin_user: Option<AdminUserConfig>,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self { admin_user: None }
    }
}

/// Configuration for bootstrapping an admin user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUserConfig {
    /// Admin username (required)
    pub username: String,
    /// Admin password in plain text (will be hashed)
    /// For security, prefer using OCTOFHIR__BOOTSTRAP__ADMIN_USER__PASSWORD env var
    pub password: String,
    /// Admin email address (optional)
    #[serde(default)]
    pub email: Option<String>,
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
