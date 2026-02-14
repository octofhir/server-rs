use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProfileConfig {
    pub server: Option<String>,
    pub format: Option<String>,
}

pub type ConfigFile = HashMap<String, ProfileConfig>;

fn config_dir() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".octofhir");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn load_all() -> Result<ConfigFile> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(ConfigFile::new());
    }
    let content = fs::read_to_string(&path)?;
    let cfg: ConfigFile = toml::from_str(&content)?;
    Ok(cfg)
}

pub fn load_profile(profile: &str) -> Result<ProfileConfig> {
    let all = load_all()?;
    Ok(all.into_iter().find(|(k, _)| k == profile).map(|(_, v)| v).unwrap_or_default())
}

pub fn save_profile(profile: &str, config: &ProfileConfig) -> Result<()> {
    let mut all = load_all()?;
    all.insert(profile.to_string(), ProfileConfig {
        server: config.server.clone(),
        format: config.format.clone(),
    });
    let content = toml::to_string_pretty(&all)?;
    fs::write(config_path()?, content)?;
    Ok(())
}

pub fn resolve_server(cli_server: &Option<String>, profile: &str) -> Result<String> {
    // 1. --server flag / OCTOFHIR_URL env
    if let Some(s) = cli_server {
        return Ok(s.clone());
    }
    // 2. config.toml profile
    let cfg = load_profile(profile)?;
    if let Some(s) = cfg.server {
        return Ok(s);
    }
    // 3. Stored credentials for this profile
    if let Ok(Some(creds)) = crate::auth::load_credentials(profile) {
        return Ok(creds.server().to_string());
    }
    anyhow::bail!(
        "No server URL configured. Use --server, set OCTOFHIR_URL env var, or run: octofhir login --server <url>"
    )
}
