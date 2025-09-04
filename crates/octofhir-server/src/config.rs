use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)] pub server: ServerConfig,
    #[serde(default)] pub storage: StorageConfig,
    #[serde(default)] pub search: SearchSettings,
    #[serde(default)] pub logging: LoggingConfig,
    #[serde(default)] pub otel: OtelConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            search: SearchSettings::default(),
            logging: LoggingConfig::default(),
            otel: OtelConfig::default(),
        }
    }
}

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
            return Err(format!("logging.level must be one of {:?}", valid_levels));
        }
        // OTEL validation
        if self.otel.enabled && self.otel.endpoint.as_deref().unwrap_or("").is_empty() {
            return Err("otel.enabled=true requires otel.endpoint".into());
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

    pub fn read_timeout(&self) -> Duration { Duration::from_millis(self.server.read_timeout_ms as u64) }
    pub fn write_timeout(&self) -> Duration { Duration::from_millis(self.server.write_timeout_ms as u64) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")] pub host: String,
    #[serde(default = "default_port")] pub port: u16,
    #[serde(default = "default_read_timeout_ms")] pub read_timeout_ms: u32,
    #[serde(default = "default_write_timeout_ms")] pub write_timeout_ms: u32,
    #[serde(default = "default_body_limit")] pub body_limit_bytes: usize,
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 8080 }
fn default_read_timeout_ms() -> u32 { 15_000 }
fn default_write_timeout_ms() -> u32 { 15_000 }
fn default_body_limit() -> usize { 1024 * 1024 }

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            read_timeout_ms: default_read_timeout_ms(),
            write_timeout_ms: default_write_timeout_ms(),
            body_limit_bytes: default_body_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default)] pub backend: StorageBackend,
    #[serde(default)] pub memory_limit_bytes: Option<usize>,
    #[serde(default)] pub preallocate_items: Option<usize>,
}

impl Default for StorageConfig {
    fn default() -> Self { Self { backend: StorageBackend::InMemoryPapaya, memory_limit_bytes: None, preallocate_items: None } }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageBackend { InMemoryPapaya }

impl Default for StorageBackend { fn default() -> Self { StorageBackend::InMemoryPapaya } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSettings {
    #[serde(default = "default_search_default")] pub default_count: usize,
    #[serde(default = "default_search_max")] pub max_count: usize,
}
fn default_search_default() -> usize { 10 }
fn default_search_max() -> usize { 100 }
impl Default for SearchSettings {
    fn default() -> Self { Self { default_count: default_search_default(), max_count: default_search_max() } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")] pub level: String,
}
fn default_log_level() -> String { "info".into() }
impl Default for LoggingConfig {
    fn default() -> Self { Self { level: default_log_level() } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelConfig {
    #[serde(default)] pub enabled: bool,
    #[serde(default)] pub endpoint: Option<String>,
    #[serde(default)] pub sample_ratio: Option<f64>,
}
impl Default for OtelConfig {
    fn default() -> Self { Self { enabled: false, endpoint: None, sample_ratio: None } }
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

    pub fn load_config_with_default_path<P: AsRef<Path>>(path: Option<P>) -> Result<AppConfig, String> {
        let p = path.as_ref().map(|p| p.as_ref().to_string_lossy().to_string());
        load_config(p.as_deref())
    }
}

pub mod shared {
    use super::AppConfig;
    use std::sync::{Arc, RwLock, OnceLock};

    static SHARED: OnceLock<Arc<RwLock<AppConfig>>> = OnceLock::new();

    pub fn set_shared(cfg: Arc<RwLock<AppConfig>>) {
        let _ = SHARED.set(cfg);
    }

    pub fn get() -> Option<&'static Arc<RwLock<AppConfig>>> {
        SHARED.get()
    }

    pub fn with_config<R>(f: impl FnOnce(&AppConfig) -> R) -> Option<R> {
        get().and_then(|arc| arc.read().ok().map(|g| f(&*g)))
    }
}
