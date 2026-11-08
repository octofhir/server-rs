//! Server command line parsed before configuration, logging or network initialization.
//!
//! # Contents
//! Help/version output and explicit configuration-path precedence.
//!
//! # Invariants
//! Help/version never require a valid deployment configuration or PostgreSQL.
//! CLI configuration wins over dotenv/environment, then the default file path.
use clap::Parser;
#[derive(Parser)]
#[command(
    name = "octofhir-server",
    version,
    about = "FHIR server with an embedded administration console"
)]
pub struct Cli {
    /// Path to the server TOML configuration (otherwise OCTOFHIR_CONFIG or octofhir.toml).
    #[arg(short,long,value_name="PATH",value_parser=nonempty)]
    config: Option<String>,
}
fn nonempty(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err("configuration path must not be empty".into())
    } else {
        Ok(value.into())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    CliArgument,
    EnvironmentVariable,
    Default,
}
impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CliArgument => "CLI argument (--config)",
            Self::EnvironmentVariable => "environment variable (OCTOFHIR_CONFIG)",
            Self::Default => "default",
        })
    }
}
impl Cli {
    pub fn config_path(self, environment: Option<String>) -> (String, ConfigSource) {
        if let Some(path) = self.config {
            return (path, ConfigSource::CliArgument);
        }
        if let Some(path) = environment.filter(|v| !v.is_empty()) {
            return (path, ConfigSource::EnvironmentVariable);
        }
        ("octofhir.toml".into(), ConfigSource::Default)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_wins_over_environment_and_default() {
        assert_eq!(
            Cli::try_parse_from(["server", "--config", "cli.toml"])
                .unwrap()
                .config_path(Some("env.toml".into())),
            ("cli.toml".into(), ConfigSource::CliArgument)
        );
        assert_eq!(
            Cli::try_parse_from(["server"])
                .unwrap()
                .config_path(Some("env.toml".into())),
            ("env.toml".into(), ConfigSource::EnvironmentVariable)
        );
        assert_eq!(
            Cli::try_parse_from(["server"]).unwrap().config_path(None),
            ("octofhir.toml".into(), ConfigSource::Default)
        );
    }
    #[test]
    fn missing_config_and_unknown_options_are_errors() {
        assert!(Cli::try_parse_from(["server", "--config"]).is_err());
        assert!(Cli::try_parse_from(["server", "--unsupported"]).is_err());
    }
}
