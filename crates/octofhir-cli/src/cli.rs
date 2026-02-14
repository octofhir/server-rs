use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "octofhir")]
#[command(about = "OctoFHIR CLI — interact with any FHIR server")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Server base URL (overrides config and OCTOFHIR_URL env var)
    #[arg(short, long, global = true, env = "OCTOFHIR_URL")]
    pub server: Option<String>,

    /// Config profile name
    #[arg(short, long, global = true, env = "OCTOFHIR_PROFILE", default_value = "default")]
    pub profile: String,

    /// Output format
    #[arg(short, long, global = true)]
    pub format: Option<OutputFormat>,
}

#[derive(Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Json,
    Yaml,
    Table,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Login to a FHIR server
    Login(LoginArgs),
    /// Logout (remove stored credentials)
    Logout,
    /// Show current auth info
    Whoami,
    /// Read a resource by reference (e.g. Patient/123)
    Get(GetArgs),
    /// Create a new resource
    Create(CreateArgs),
    /// Update a resource
    Update(UpdateArgs),
    /// Delete a resource
    Delete(DeleteArgs),
    /// View resource history
    History(HistoryArgs),
    /// Search for resources
    Search(SearchArgs),
    /// Get server CapabilityStatement
    Metadata,
    /// Check server health
    Status,
    /// Manage CLI configuration
    Config(ConfigArgs),
}

#[derive(Clone, Copy, ValueEnum, Default)]
pub enum AuthFlow {
    /// HTTP Basic Auth (default) — stores username:password
    #[default]
    Basic,
    /// OAuth 2.0 — obtains and stores a Bearer token
    OAuth,
}

#[derive(clap::Args)]
pub struct LoginArgs {
    /// Username
    #[arg(short, long)]
    pub username: Option<String>,
    /// Password
    #[arg(long)]
    pub password: Option<String>,
    /// Auth flow to use
    #[arg(long, default_value = "basic")]
    pub auth_flow: AuthFlow,
    /// OAuth client ID (required for --auth-flow oauth with client_credentials, optional for password grant)
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth client secret (triggers client_credentials grant)
    #[arg(long)]
    pub client_secret: Option<String>,
}

#[derive(clap::Args)]
pub struct GetArgs {
    /// Resource reference (e.g. Patient/123)
    pub reference: String,
}

#[derive(clap::Args)]
pub struct CreateArgs {
    /// Resource type (e.g. Patient)
    pub resource_type: String,
    /// Path to JSON file (reads from stdin if omitted)
    #[arg(long)]
    pub file: Option<String>,
}

#[derive(clap::Args)]
pub struct UpdateArgs {
    /// Resource reference (e.g. Patient/123)
    pub reference: String,
    /// Path to JSON file (reads from stdin if omitted)
    #[arg(long)]
    pub file: Option<String>,
}

#[derive(clap::Args)]
pub struct DeleteArgs {
    /// Resource reference (e.g. Patient/123)
    pub reference: String,
}

#[derive(clap::Args)]
pub struct HistoryArgs {
    /// Resource reference (e.g. Patient/123)
    pub reference: String,
}

#[derive(clap::Args)]
pub struct SearchArgs {
    /// Resource type (e.g. Patient)
    pub resource_type: String,
    /// Search parameters as key=value pairs (e.g. name=Smith birthdate=gt1990-01-01)
    pub params: Vec<String>,
    /// Number of results per page
    #[arg(long)]
    pub count: Option<u32>,
}

#[derive(clap::Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current config
    Show,
    /// Set config value
    Set(ConfigSetArgs),
}

#[derive(clap::Args)]
pub struct ConfigSetArgs {
    /// Key to set (server, format)
    pub key: String,
    /// Value
    pub value: String,
}
