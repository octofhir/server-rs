pub use crate::package_source::S3PackageSource;
use octofhir_auth::config::AuthConfig;
use octofhir_search::TerminologyConfig;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

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
    /// Named DICOMweb metadata connections; empty disables integration.
    #[serde(default)]
    pub dicom: crate::dicom::DicomConfig,
    /// Create missing provider resources before acquiring packages; existing DB records win.
    #[serde(default)]
    pub storage_providers: Vec<crate::storage_providers::StorageProvider>,
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
    /// GraphQL configuration
    #[serde(default)]
    pub graphql: GraphQLConfig,
    /// Bulk export configuration ($export)
    #[serde(default)]
    pub bulk_export: BulkExportConfig,
    /// Bootstrap configuration (initial admin user, default data)
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
    /// Audit trail configuration
    #[serde(default)]
    pub audit: AuditConfig,
    /// SQL on FHIR configuration (ViewDefinition editor, $run/$export operations)
    #[serde(default)]
    pub sql_on_fhir: SqlOnFhirConfig,
    /// CQL evaluation configuration ($cql, $evaluate-measure operations)
    #[serde(default)]
    pub cql: octofhir_cql_service::CqlConfig,
    /// Bulk import configuration ($import operation)
    #[serde(default)]
    pub bulk_import: BulkImportConfig,
    /// Bulk submit configuration ($bulk-submit, $bulk-submit-status)
    #[serde(default)]
    pub bulk_submit: BulkSubmitConfig,
    /// Model Context Protocol endpoint (POST /mcp)
    #[serde(default)]
    pub mcp: McpConfig,
    /// What the CapabilityStatement declares beyond the base specification
    #[serde(default)]
    pub capability: CapabilityConfig,

    /// FHIR Mapping Language: StructureMap `$transform`
    #[serde(default)]
    pub fml: FmlConfig,

    /// FHIR Subscriptions (topic-based notifications)
    #[serde(default)]
    pub subscriptions: SubscriptionsConfig,

    /// Outbound messages: email, Telegram and webhooks through `NotificationProvider`s
    #[serde(default)]
    pub notifications: NotificationsConfig,

    /// `/metrics` endpoint and the Prometheus server the console's dashboards read from
    #[serde(default)]
    pub metrics: MetricsConfig,
}

// Default derived via field defaults

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        let egress = crate::egress::EgressPolicy::from_server_config(&self.server)?;
        self.dicom.validate(&egress, &self.fhir.version)?;
        let mut provider_ids = std::collections::BTreeSet::new();
        for provider in &self.storage_providers {
            // Existing database resources win; validate seed connection settings only if inserted.
            crate::storage_providers::model::validate_id(&provider.id)?;
            if !provider_ids.insert(&provider.id) {
                return Err("Duplicate bootstrap storage provider id".into());
            }
        }
        for source in &self.packages.load {
            if let PackageSpec::S3 { s3, .. } = source {
                s3.validate()?;
            }
        }
        if let Some(email) = &self.subscriptions.email
            && email.smtp_host.is_none()
            && email.sendgrid_api_key.is_none()
        {
            return Err("subscriptions.email needs smtp_host or sendgrid_api_key".into());
        }
        // Server validations
        if self.server.port == 0 {
            return Err("server.port must be > 0".into());
        }
        if self.server.read_timeout_ms == 0
            || self.server.write_timeout_ms == 0
            || self.server.bundle_timeout_ms == 0
        {
            return Err("server timeouts must be > 0".into());
        }
        if self.server.max_conditional_reference_candidates == 0
            || self.server.max_conditional_reference_assignments == 0
            || self.server.max_conditional_reference_total_candidates == 0
        {
            return Err("conditional reference resolution limits must be > 0".into());
        }
        let import = &self.bulk_import;
        if import.batch_size == 0
            || import.max_parallel_resources == 0
            || import.max_concurrent_jobs == 0
            || import.max_sources == 0
            || import.max_errors == 0
            || import.max_source_bytes == 0
            || import.max_line_bytes == 0
            || import.max_batch_bytes < import.max_line_bytes
        {
            return Err(
                "bulk_import limits must be positive and max_batch_bytes >= max_line_bytes".into(),
            );
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
        if let Some(prometheus) = &self.metrics.prometheus {
            prometheus.validate()?;
        }
        // OTEL validation
        if self.otel.enabled && self.otel.endpoint.as_deref().unwrap_or("").is_empty() {
            return Err("otel.enabled=true requires otel.endpoint".into());
        }
        // FHIR version validation
        self.fhir.release()?;
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
        // Auth validation (always required)
        self.auth
            .validate()
            .map_err(|e| format!("auth config error: {e}"))?;

        // Validate backend client config if provided
        if let Some(ref backend_client) = self.bootstrap.backend_client {
            if backend_client.client_id.is_empty() {
                return Err("bootstrap.backend_client.client_id cannot be empty".into());
            }
            if backend_client.scopes.is_empty() {
                return Err("bootstrap.backend_client.scopes cannot be empty".into());
            }
            // Enforce system/* scopes for backend services
            for scope in &backend_client.scopes {
                if !scope.starts_with("system/") {
                    return Err(format!(
                        "Backend client scope '{}' must start with 'system/' (backend services only support system-level access)",
                        scope
                    ));
                }
            }
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
    pub fn bundle_timeout(&self) -> Duration {
        Duration::from_millis(self.server.bundle_timeout_ms as u64)
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
    #[serde(default = "default_bundle_timeout_ms")]
    pub bundle_timeout_ms: u32,
    /// Maximum identities per predicate when solving a dependent reference graph.
    #[serde(default = "default_conditional_reference_candidates")]
    pub max_conditional_reference_candidates: usize,
    /// Maximum candidate identities across all predicates in one graph.
    #[serde(default = "default_conditional_reference_total_candidates")]
    pub max_conditional_reference_total_candidates: usize,
    /// Maximum complete graph assignments checked before returning too-costly.
    #[serde(default = "default_conditional_reference_assignments")]
    pub max_conditional_reference_assignments: usize,
    #[serde(default = "default_body_limit")]
    pub body_limit_bytes: usize,
    #[serde(default)]
    pub compression: bool,
    /// Let subscription rest-hooks and `$import` reach any loopback, private or link-local
    /// address. On by default. Clients choose those URLs, so on a shared deployment turn it off
    /// and list the internal services that may be called in `outbound_allowlist`.
    #[serde(default = "default_allow_private_outbound")]
    pub allow_private_outbound: bool,
    /// With `allow_private_outbound = false`: private networks and hosts that rest-hooks and
    /// `$import` may still reach, e.g. a source service on the same Docker network:
    /// `["172.18.0.0/16", "etl-service", "*.internal"]`.
    #[serde(default)]
    pub outbound_allowlist: Vec<String>,
}

fn default_allow_private_outbound() -> bool {
    true
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
fn default_bundle_timeout_ms() -> u32 {
    600_000
}
fn default_conditional_reference_candidates() -> usize {
    256
}
fn default_conditional_reference_total_candidates() -> usize {
    10_000
}
fn default_conditional_reference_assignments() -> usize {
    10_000
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
            bundle_timeout_ms: default_bundle_timeout_ms(),
            max_conditional_reference_candidates: default_conditional_reference_candidates(),
            max_conditional_reference_assignments: default_conditional_reference_assignments(),
            max_conditional_reference_total_candidates:
                default_conditional_reference_total_candidates(),
            body_limit_bytes: default_body_limit(),
            compression: false,
            allow_private_outbound: default_allow_private_outbound(),
            outbound_allowlist: Vec::new(),
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

    /// Optional read replica configuration.
    /// When set, read operations (search, read, vread, history) are routed
    /// to this replica pool while writes stay on the primary.
    #[serde(default)]
    pub read_replica: Option<ReadReplicaConfig>,

    /// Resource types that get a whole-document GIN index on `resource`.
    ///
    /// Omit the key and every type gets one. The document GIN answers a search
    /// on any element, so it is what keeps a search on a parameter outside
    /// `search.indexed_params` from turning into a sequential scan. It is also
    /// the largest index on a resource table and every write maintains it, so a
    /// deployment that knows its query mix can narrow the list — or set it to an
    /// empty list to drop the index entirely.
    #[serde(default)]
    pub document_gin_resource_types: Option<Vec<String>>,

    /// Give every resource table a generated `profile` column (from
    /// `meta.profile`) with partial covering and GIN indexes.
    ///
    /// On by default: it backs `_profile` search and the `targetProfile`
    /// conformance fast path. The column is created with the table, so changing
    /// this takes effect only for a recreated database.
    #[serde(default = "default_profile_column")]
    pub profile_column: bool,

    /// PostgreSQL settings applied to every pooled connection (primary and
    /// read replica) through the startup packet.
    ///
    /// Defaults to `jit = "off"` and `max_parallel_workers_per_gather = "0"`:
    /// FHIR API statements are short and concurrent, so JIT compilation and
    /// parallel workers cost CPU that the other connections need. Neither
    /// changes query results. Setting the table replaces both defaults.
    #[serde(default = "octofhir_db_postgres::default_session_settings")]
    pub session_settings: BTreeMap<String, String>,
}

/// Read replica configuration. Only `url` is required;
/// pool settings default to the primary's values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadReplicaConfig {
    /// Connection URL for the read replica.
    pub url: String,

    /// Pool size for the replica (defaults to primary pool_size).
    #[serde(default)]
    pub pool_size: Option<u32>,

    /// Connection timeout in milliseconds (defaults to primary's).
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,

    /// Idle timeout in milliseconds (defaults to primary's).
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
fn default_profile_column() -> bool {
    true
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
            read_replica: None,
            document_gin_resource_types: None,
            profile_column: default_profile_column(),
            session_settings: octofhir_db_postgres::default_session_settings(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSettings {
    /// Maximum include expansion by resources, serialized bytes, and depth.
    #[serde(default)]
    pub include_limits: octofhir_db_postgres::queries::search::IncludeLimits,
    #[serde(default = "default_search_default")]
    pub default_count: usize,
    #[serde(default = "default_search_max")]
    pub max_count: usize,
    /// Query cache capacity for search parameter registry
    /// Default: 1000
    #[serde(default = "default_search_cache_capacity")]
    pub cache_capacity: usize,
    /// Allow internal `_debug=search-plan` search plan collection.
    /// Default: false
    #[serde(default)]
    pub allow_debug_search_plan: bool,
    /// Allow internal `_debug=search-explain-analyze`.
    ///
    /// Default is false because EXPLAIN ANALYZE executes the search query.
    #[serde(default)]
    pub allow_debug_search_explain_analyze: bool,
    /// Search parameters to build functional in-place indexes for at bootstrap,
    /// each as `"ResourceType.code"` (e.g. `"Patient.birthdate"`). Indexes are
    /// created once when tables + registry are ready; nothing is created at
    /// runtime. Defaults to a popular set; override per deployment (env
    /// `OCTOFHIR__SEARCH__INDEXED_PARAMS`) to index only what you search.
    #[serde(default = "default_indexed_params")]
    pub indexed_params: Vec<String>,
    /// Quantity search parameters that get a GiST index over their canonical
    /// UCUM values, each as `"ResourceType.code"` (e.g.
    /// `"Observation.value-quantity"`). Unit-aware matching (1 g finds 1000 mg)
    /// works without it, but a search that names the UCUM system then scans the
    /// table, while every write to an indexed type pays for the index. Empty by
    /// default; env `OCTOFHIR__SEARCH__UCUM_INDEXED_PARAMS`.
    #[serde(default)]
    pub ucum_indexed_params: Vec<String>,
    /// Maximum number of concepts a token `:in`/`:not-in`/`:above`/`:below`
    /// ValueSet/hierarchy may expand to before the request is rejected. Each
    /// expanded code becomes an OR branch in the rewritten query, so an
    /// unbounded expansion would hang Postgres. Env:
    /// `OCTOFHIR__SEARCH__MAX_VALUESET_EXPANSION`.
    #[serde(default = "default_max_valueset_expansion")]
    pub max_valueset_expansion: usize,
    /// Targeted PARTIAL composite indexes, each pinned to one token value — e.g.
    /// `Observation.code-value-quantity` restricted to `code = 8867-4`. A composite
    /// search for that exact token is then served by a tiny btree pre-filtered to it
    /// (Postgres partial index), instead of BitmapAnd-ing two large bitmaps. Empty by
    /// default (no hardcoded codes); set per deployment via TOML
    /// `[[search.composite_index]]` or env `OCTOFHIR__SEARCH__COMPOSITE_INDEXES`
    /// (`ResourceType.param=system|code`, system optional: `...=|8867-4`).
    #[serde(default)]
    pub composite_index: Vec<CompositeIndexSpec>,
    /// Upper bound on searches running against PostgreSQL at the same time.
    ///
    /// Searches beyond the bound wait in FIFO order for a slot. Keeping the
    /// number of active backends close to the database's CPU count avoids
    /// oversubscribing it: under a container CPU quota, oversubscription shows
    /// up as stalls of up to one quota period on whichever queries are in
    /// flight. A slot also covers the request's round trips, so start at 2–3×
    /// the database CPUs: against a 4-CPU PostgreSQL quota, 30 concurrent
    /// searches throttled 43% of quota periods (p99 45 ms); 12 slots kept the
    /// same throughput at p99 26 ms, while 5 slots lost 15% of it.
    /// Writes are not limited. Unset (the default) means no bound beyond the
    /// pool size. Env: `OCTOFHIR__SEARCH__MAX_CONCURRENT_QUERIES`.
    #[serde(default)]
    pub max_concurrent_queries: Option<usize>,
}

/// One targeted partial composite index: index the quantity component of `param`
/// on `resource_type`, restricted (index `WHERE`) to rows whose token component
/// equals `code` (optionally within `system`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompositeIndexSpec {
    pub resource_type: String,
    pub param: String,
    #[serde(default)]
    pub system: Option<String>,
    pub code: String,
}

impl CompositeIndexSpec {
    /// Parse a compact env entry `ResourceType.param=system|code` (system optional:
    /// `ResourceType.param=|code` or `ResourceType.param=code`). Returns None if the
    /// shape is malformed.
    pub fn parse_compact(entry: &str) -> Option<Self> {
        let (left, right) = entry.split_once('=')?;
        let (resource_type, param) = left.split_once('.')?;
        let (system, code) = match right.split_once('|') {
            Some((sys, code)) => {
                let sys = sys.trim();
                ((!sys.is_empty()).then(|| sys.to_string()), code.trim())
            }
            None => (None, right.trim()),
        };
        if resource_type.is_empty() || param.is_empty() || code.is_empty() {
            return None;
        }
        Some(Self {
            resource_type: resource_type.trim().to_string(),
            param: param.trim().to_string(),
            system,
            code: code.to_string(),
        })
    }
}
fn default_max_valueset_expansion() -> usize {
    octofhir_search::terminology_preprocess::DEFAULT_MAX_EXPANSION_SIZE
}
fn default_indexed_params() -> Vec<String> {
    [
        "Patient.birthdate",
        "Patient.death-date",
        "Observation.date",
        "Encounter.date",
        "Condition.recorded-date",
        "Condition.onset-date",
        "Procedure.date",
        "MedicationRequest.authoredon",
        "DiagnosticReport.date",
        "Immunization.date",
        "AllergyIntolerance.date",
        "CarePlan.date",
        "DocumentReference.date",
        "ExplanationOfBenefit.created",
        "Claim.created",
        "Patient.name",
        "Patient.family",
        "Patient.given",
        "Practitioner.name",
        "Organization.name",
        "Observation.subject",
        "Observation.encounter",
        "Observation.performer",
        "Observation.patient",
        "Encounter.subject",
        "Encounter.patient",
        "Encounter.participant",
        "MedicationRequest.subject",
        "MedicationRequest.patient",
        "MedicationRequest.encounter",
        "MedicationRequest.requester",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}
fn default_search_default() -> usize {
    10
}
fn default_search_max() -> usize {
    100
}
fn default_search_cache_capacity() -> usize {
    1000
}
impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            include_limits: Default::default(),
            default_count: default_search_default(),
            max_count: default_search_max(),
            cache_capacity: default_search_cache_capacity(),
            allow_debug_search_plan: false,
            allow_debug_search_explain_analyze: false,
            indexed_params: default_indexed_params(),
            ucum_indexed_params: Vec::new(),
            max_valueset_expansion: default_max_valueset_expansion(),
            composite_index: Vec::new(),
            max_concurrent_queries: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Log output format: "json" or "text"
    /// Default: "json" (structured JSON for machine parsing, lower CPU overhead)
    #[serde(default = "default_log_format")]
    pub format: String,
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "json".into()
}
impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
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
impl FhirSettings {
    pub fn release(&self) -> Result<octofhir_core::fhir::FhirVersion, String> {
        self.version.parse().map_err(|_| {
            "fhir.version must be R4 (4.0.1), R4B (4.3.0), R5 (5.0.0), or R6 (6.0.0)".to_string()
        })
    }
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
    /// Never contact package registries or direct package URLs.
    /// Explicit pinned S3 sources remain allowed. Exact dependency versions must
    /// be installed, supplied by configured sources, or present in the archive cache.
    #[serde(default)]
    pub offline: bool,
    /// List of package specs to load. Shorthand examples:
    /// - "hl7.fhir.r4b.core#4.3.0"
    /// - "hl7.terminology#5.5.0"
    ///   Optionally supports absolute/relative paths via table form in TOML.
    ///   When specified as tables, fields: { id = "...", version = "...", path = "..." }
    #[serde(default)]
    pub load: Vec<PackageSpec>,
    /// Optional base directory for package caches. Defaults to `.fhir`.
    /// Relative paths resolve against the configuration file directory.
    /// Note: Package data is stored in PostgreSQL's 'fcm' schema.
    #[serde(default)]
    pub path: Option<String>,
}

// Default derived

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum PackageSpec {
    /// Pinned object acquisition, including private S3-compatible endpoints.
    S3 {
        id: Option<String>,
        version: Option<String>,
        s3: S3PackageSource,
    },
    /// Shorthand: "package_id#version" or just "package_id"
    Simple(String),
    /// Expanded form for clarity, filesystem path loading, or URL-based downloads
    Table {
        id: Option<String>,
        version: Option<String>,
        path: Option<String>,
        /// Optional direct URL to download the package tarball from.
        /// If provided, this URL will be used instead of querying the registry.
        /// Useful for packages not in the registry or CI builds.
        /// Example: "<https://build.fhir.org/ig/FHIR/sql-on-fhir-v2/package.tgz>"
        url: Option<String>,
    },
}

impl PackageSpec {
    /// `name@version` or a folder, as package tools take it; `None` without either.
    pub fn reference(&self) -> Option<String> {
        match self {
            Self::Simple(spec) => Some(spec.replacen('#', "@", 1)),
            Self::S3 { s3, .. } => Some(format!("s3://{}/{}", s3.bucket, s3.key)),
            Self::Table {
                path: Some(path), ..
            } => Some(path.clone()),
            Self::Table {
                id: Some(id),
                version,
                ..
            } => Some(match version {
                Some(version) => format!("{id}@{version}"),
                None => id.clone(),
            }),
            Self::Table { .. } => None,
        }
    }
}

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSettings {
    /// Allow clients to skip validation via X-Skip-Validation header
    /// Default: false (disabled for security)
    #[serde(default = "default_allow_skip_validation")]
    pub allow_skip_validation: bool,

    /// Skip reference validation (both type checking and existence checking)
    /// Default: false (reference validation is enabled)
    #[serde(default)]
    pub skip_reference_validation: bool,

    /// Validate that references conform to their declared `targetProfile`.
    /// When enabled, each typed reference is dereferenced and the referenced
    /// resource is validated against the profile(s) it must conform to.
    /// Default: false — this performs a fetch + full re-validation per typed
    /// reference and is intentionally off (also keeps write benchmarks honest).
    #[serde(default)]
    pub check_target_profile: bool,

    /// Dereference external absolute-URL references over the network for
    /// `targetProfile` conformance. Subject to an SSRF guard (private/loopback
    /// targets are always blocked). Default: false.
    #[serde(default)]
    pub fetch_external_references: bool,

    /// Per-request timeout (ms) for external reference fetches.
    #[serde(default = "default_reference_fetch_timeout_ms")]
    pub reference_fetch_timeout_ms: u64,

    /// Maximum recursion depth for transitive `targetProfile` conformance.
    #[serde(default = "default_max_reference_depth")]
    pub max_reference_depth: usize,
}

fn default_allow_skip_validation() -> bool {
    false
}

fn default_reference_fetch_timeout_ms() -> u64 {
    5000
}

fn default_max_reference_depth() -> usize {
    5
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            allow_skip_validation: default_allow_skip_validation(),
            skip_reference_validation: false,
            check_target_profile: false,
            fetch_external_references: false,
            reference_fetch_timeout_ms: default_reference_fetch_timeout_ms(),
            max_reference_depth: default_max_reference_depth(),
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

    /// Resource read cache TTL in seconds (0 to disable)
    #[serde(default = "default_resource_ttl_secs")]
    pub resource_ttl_secs: u64,
}

fn default_terminology_ttl_secs() -> u64 {
    3600 // 1 hour
}

fn default_local_cache_max_entries() -> usize {
    10000
}

fn default_resource_ttl_secs() -> u64 {
    60 // 1 minute default TTL for resource cache
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            terminology_ttl_secs: default_terminology_ttl_secs(),
            local_cache_max_entries: default_local_cache_max_entries(),
            resource_ttl_secs: default_resource_ttl_secs(),
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
                // Allow EXPLAIN/EXPLAIN ANALYZE for SELECT queries
                let check_query = if query_upper.starts_with("EXPLAIN") {
                    query_upper
                        .strip_prefix("EXPLAIN ANALYZE")
                        .or_else(|| query_upper.strip_prefix("EXPLAIN"))
                        .map(|s| s.trim_start())
                        .unwrap_or(&query_upper)
                } else {
                    &query_upper
                };

                // Must start with SELECT or WITH
                if !check_query.starts_with("SELECT") && !check_query.starts_with("WITH") {
                    return Err(
                        "Only SELECT queries (and EXPLAIN of SELECT) are allowed in readonly mode"
                            .to_string(),
                    );
                }
                // Check for dangerous keywords that could be in subqueries
                let forbidden = [
                    "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT",
                    "REVOKE",
                ];
                for kw in forbidden {
                    if check_query.contains(kw) {
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

/// GraphQL configuration for FHIR GraphQL API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLConfig {
    /// Enable GraphQL endpoint
    /// Default: true
    #[serde(default = "default_graphql_enabled")]
    pub enabled: bool,

    /// Enable introspection queries (__schema, __type)
    /// May want to disable in production for security
    /// Default: true
    #[serde(default = "default_graphql_introspection")]
    pub introspection: bool,

    /// Enable GraphQL subscriptions for real-time resource changes
    /// Requires WebSocket support
    /// Default: false
    #[serde(default = "default_graphql_subscriptions")]
    pub subscriptions: bool,

    /// Maximum query depth to prevent deeply nested queries
    /// Default: 15
    #[serde(default = "default_graphql_max_depth")]
    pub max_depth: usize,

    /// Maximum query complexity score
    /// Default: 500
    #[serde(default = "default_graphql_max_complexity")]
    pub max_complexity: usize,

    /// FHIR resource types to expose via GraphQL
    /// If empty, all resource types are exposed
    /// Default: [] (all resources)
    #[serde(default)]
    pub resource_types: Vec<String>,
}

fn default_graphql_enabled() -> bool {
    true
}

fn default_graphql_introspection() -> bool {
    true
}

fn default_graphql_subscriptions() -> bool {
    false
}

fn default_graphql_max_depth() -> usize {
    15
}

fn default_graphql_max_complexity() -> usize {
    500
}

impl Default for GraphQLConfig {
    fn default() -> Self {
        Self {
            enabled: default_graphql_enabled(),
            introspection: default_graphql_introspection(),
            subscriptions: default_graphql_subscriptions(),
            max_depth: default_graphql_max_depth(),
            max_complexity: default_graphql_max_complexity(),
            resource_types: Vec::new(),
        }
    }
}

/// Conformance claims of the CapabilityStatement. Installing a package claims nothing;
/// an operator who has tested the server against an IG names it here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityConfig {
    /// Canonical URLs of CapabilityStatements from loaded packages. Each goes into
    /// `instantiates`, and its `supportedProfile` lists join those of each resource.
    #[serde(default)]
    pub instantiates: Vec<String>,
}

/// Model Context Protocol endpoint. Tools run as the caller's own REST requests, so
/// the token decides what an agent can reach.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Serve `POST /mcp`. Default: true.
    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,
    /// Offer `write_resource`, labelled and recorded as AI asserted. Default: false.
    #[serde(default)]
    pub allow_writes: bool,
    /// Offer tools over the loaded packages: find profiles and extensions, describe a profile,
    /// run a view over given resources. Loads the packages again in memory. Default: false.
    #[serde(default)]
    pub toolbox: bool,
}

fn default_mcp_enabled() -> bool {
    true
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_enabled(),
            allow_writes: false,
            toolbox: false,
        }
    }
}

/// Version of the Bulk Data Access IG that `$export` follows.
///
/// STU 4 removes parts of STU 3 rather than adding to it, so one endpoint cannot
/// be exact for both: GET kick-off, comma-delimited values and the `request`
/// manifest field are gone, `error` became `outcome`, and `includeAssociatedData`
/// became a Coding. `$bulk-submit` only adds operations and is independent of this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkSpecVersion {
    /// Published STU 3 (3.0.0): GET and POST kick-off, `error` in the manifest.
    #[default]
    Stu3,
    /// STU 4 (4.0.0-ballot): POST kick-off only, `outcome` in the manifest.
    Stu4,
}

impl BulkSpecVersion {
    /// Version of the OperationDefinitions this mode implements.
    pub fn ig_version(self) -> &'static str {
        match self {
            Self::Stu3 => "3.0.0",
            Self::Stu4 => "4.0.0-ballot",
        }
    }
}

/// Bulk Data Export configuration (FHIR Bulk Data Access IG)
///
/// Configures the $export operation for system-level, patient-level,
/// and group-level bulk data exports in NDJSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkExportConfig {
    /// Enable bulk export functionality
    /// Default: true
    #[serde(default = "default_bulk_export_enabled")]
    pub enabled: bool,

    /// Directory path for storing export files
    /// Default: "./exports" (relative to working directory)
    #[serde(default = "default_bulk_export_path")]
    pub export_path: String,

    /// Maximum concurrent export jobs
    /// Default: 5
    #[serde(default = "default_bulk_export_max_concurrent")]
    pub max_concurrent_jobs: usize,

    /// Export file retention period in hours
    /// Files older than this will be cleaned up
    /// Default: 24 hours
    #[serde(default = "default_bulk_export_retention_hours")]
    pub retention_hours: u64,

    /// Maximum resources per NDJSON file before splitting
    /// Default: 100000
    #[serde(default = "default_bulk_export_max_resources_per_file")]
    pub max_resources_per_file: usize,

    /// Batch size for streaming resources from database
    /// Default: 1000
    #[serde(default = "default_bulk_export_batch_size")]
    pub batch_size: usize,

    /// Bulk Data Access IG version `$export` follows: "stu3" or "stu4".
    /// Default: "stu3", the published version.
    #[serde(default)]
    pub spec_version: BulkSpecVersion,

    /// Output files per manifest page when a client sets `allowPartialManifests`.
    /// Default: 100
    #[serde(default = "default_bulk_export_max_files_per_manifest")]
    pub max_files_per_manifest: usize,

    /// Shortest allowed gap between status polls of one job; faster polls get
    /// 429 with `Retry-After`. 0 turns the check off.
    /// Default: 1000
    #[serde(default = "default_bulk_export_min_poll_interval_ms")]
    pub min_poll_interval_ms: u64,

    /// Default resource types to export if _type parameter not specified
    /// If empty, exports all available resource types
    #[serde(default)]
    pub default_resource_types: Vec<String>,
}

fn default_bulk_export_enabled() -> bool {
    true
}

fn default_bulk_export_path() -> String {
    "./exports".to_string()
}

fn default_bulk_export_max_concurrent() -> usize {
    5
}

fn default_bulk_export_retention_hours() -> u64 {
    24
}

fn default_bulk_export_max_resources_per_file() -> usize {
    100_000
}

fn default_bulk_export_max_files_per_manifest() -> usize {
    100
}

fn default_bulk_export_min_poll_interval_ms() -> u64 {
    1000
}

fn default_bulk_export_batch_size() -> usize {
    1000
}

impl Default for BulkExportConfig {
    fn default() -> Self {
        Self {
            enabled: default_bulk_export_enabled(),
            export_path: default_bulk_export_path(),
            max_concurrent_jobs: default_bulk_export_max_concurrent(),
            retention_hours: default_bulk_export_retention_hours(),
            max_resources_per_file: default_bulk_export_max_resources_per_file(),
            batch_size: default_bulk_export_batch_size(),
            spec_version: BulkSpecVersion::default(),
            max_files_per_manifest: default_bulk_export_max_files_per_manifest(),
            min_poll_interval_ms: default_bulk_export_min_poll_interval_ms(),
            default_resource_types: Vec::new(),
        }
    }
}

/// Delivery of notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Deliver queued notifications.
    pub enabled: bool,
    /// How often the queue is looked at, in milliseconds.
    pub poll_interval_ms: u64,
    /// How many notifications one look takes.
    pub batch_size: u16,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_ms: 5_000,
            batch_size: 10,
        }
    }
}

/// FHIR Subscriptions configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionsConfig {
    /// Activate requested subscriptions and notify subscribers of matching changes.
    #[serde(default = "default_subscriptions_enabled")]
    pub enabled: bool,

    /// Provider for `email` channel subscriptions. Without it they are put in `error`.
    #[serde(default)]
    pub email: Option<SubscriptionEmailConfig>,
}

impl Default for SubscriptionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_subscriptions_enabled(),
            email: None,
        }
    }
}

fn default_subscriptions_enabled() -> bool {
    true
}

/// Outgoing mail for `email` channel subscriptions: an SMTP relay or SendGrid.
#[derive(Clone, Serialize, Deserialize)]
pub struct SubscriptionEmailConfig {
    /// Sender address of notification emails.
    pub from: String,
    /// SMTP relay host. Set this or `sendgrid_api_key`.
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// starttls (default), implicit (port 465) or none (trusted relay, local testing).
    #[serde(default)]
    pub smtp_tls: crate::subscriptions::delivery::email::SmtpTls,
    #[serde(default)]
    pub smtp_username: Option<String>,
    #[serde(default)]
    pub smtp_password: Option<String>,
    /// SendGrid API key, used when no SMTP host is set.
    #[serde(default)]
    pub sendgrid_api_key: Option<String>,
}

fn default_smtp_port() -> u16 {
    587
}

/// Credentials stay out of logs that print the configuration.
impl std::fmt::Debug for SubscriptionEmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |value: &Option<String>| value.as_ref().map(|_| "***");
        f.debug_struct("SubscriptionEmailConfig")
            .field("from", &self.from)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_tls", &self.smtp_tls)
            .field("smtp_username", &self.smtp_username)
            .field("smtp_password", &redact(&self.smtp_password))
            .field("sendgrid_api_key", &redact(&self.sendgrid_api_key))
            .finish()
    }
}

/// The `/metrics` endpoint and the console's metrics dashboards.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct MetricsConfig {
    /// When set, `/metrics` answers only requests carrying `Authorization: Bearer <token>`
    /// (configure the same token as the scrape job's `authorization.credentials`).
    #[serde(default)]
    pub endpoint_token: Option<String>,
    /// Prometheus-compatible server (Prometheus, VictoriaMetrics, Mimir, Thanos) that scrapes
    /// this server. The console's Performance dashboards exist only when it is configured.
    #[serde(default)]
    pub prometheus: Option<PrometheusConfig>,
}

/// Credentials stay out of logs that print the configuration.
impl std::fmt::Debug for MetricsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsConfig")
            .field(
                "endpoint_token",
                &self.endpoint_token.as_ref().map(|_| "***"),
            )
            .field("prometheus", &self.prometheus)
            .finish()
    }
}

/// Where the console reads metrics from, and the limits on the queries it sends.
#[derive(Clone, Serialize, Deserialize)]
pub struct PrometheusConfig {
    /// Base URL of the Prometheus HTTP API, e.g. `http://prometheus:9090`.
    pub url: String,
    /// Series selector added to every query, so dashboards only see this deployment's series,
    /// e.g. `{job="octofhir"}`. Must be a selector with label matchers only.
    #[serde(default = "default_prometheus_selector")]
    pub selector: String,
    /// The scrape interval of the job above. Steps never go below it and rate windows cover at
    /// least four scrapes.
    #[serde(default = "default_scrape_interval_secs")]
    pub scrape_interval_secs: u64,
    #[serde(default = "default_prometheus_timeout_secs")]
    pub timeout_secs: u64,
    /// Longest time range a dashboard may query.
    #[serde(default = "default_prometheus_max_range_days")]
    pub max_range_days: u64,
    /// Most points per series; longer ranges get a coarser step.
    #[serde(default = "default_prometheus_max_points")]
    pub max_points: u64,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub basic_username: Option<String>,
    #[serde(default)]
    pub basic_password: Option<String>,
    /// Extra request headers, e.g. `X-Scope-OrgID` for Mimir or Cortex.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl PrometheusConfig {
    pub fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url)
            .map_err(|e| format!("metrics.prometheus.url is not a URL: {e}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("metrics.prometheus.url must be http or https".into());
        }
        crate::prometheus::promql::parse_selector(&self.selector)
            .map_err(|e| format!("metrics.prometheus.selector: {e}"))?;
        if self.scrape_interval_secs == 0
            || self.timeout_secs == 0
            || self.max_range_days == 0
            || self.max_points < 10
        {
            return Err("metrics.prometheus limits must be positive and max_points >= 10".into());
        }
        if self.basic_username.is_some() && self.bearer_token.is_some() {
            return Err("metrics.prometheus: set bearer_token or basic_username, not both".into());
        }
        Ok(())
    }

    pub fn scrape_interval(&self) -> Duration {
        Duration::from_secs(self.scrape_interval_secs)
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    pub fn max_range(&self) -> Duration {
        Duration::from_secs(self.max_range_days * 24 * 60 * 60)
    }
}

/// Credentials stay out of logs that print the configuration.
impl std::fmt::Debug for PrometheusConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |value: &Option<String>| value.as_ref().map(|_| "***");
        f.debug_struct("PrometheusConfig")
            .field("url", &self.url)
            .field("selector", &self.selector)
            .field("scrape_interval_secs", &self.scrape_interval_secs)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_range_days", &self.max_range_days)
            .field("max_points", &self.max_points)
            .field("bearer_token", &redact(&self.bearer_token))
            .field("basic_username", &self.basic_username)
            .field("basic_password", &redact(&self.basic_password))
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn default_prometheus_selector() -> String {
    r#"{job="octofhir"}"#.to_string()
}

fn default_scrape_interval_secs() -> u64 {
    15
}

fn default_prometheus_timeout_secs() -> u64 {
    10
}

fn default_prometheus_max_range_days() -> u64 {
    30
}

fn default_prometheus_max_points() -> u64 {
    2000
}

/// A Data Provider allowed to call `$bulk-submit`, agreed out of band as the IG
/// requires: `submitter` SHALL match a system and code the Data Consumer specified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulkSubmitter {
    /// `submitter.system`
    pub system: String,
    /// `submitter.value`
    pub value: String,
    /// OAuth client that may act as this submitter. Without it any client with
    /// the required grant may; a submission always stays with the client that opened it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// This server's client id at the submitter's authorization server. With it the
    /// server accepts `oauthMetadataUrl` and fetches files under SMART Backend Services,
    /// signing with its own key (`auth.signing`, RS384 or ES384; public at `/auth/jwks`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_client_id: Option<String>,
    /// Scope requested at the submitter's authorization server.
    #[serde(default = "default_bulk_submitter_scope")]
    pub provider_scope: String,
}

fn default_bulk_submitter_scope() -> String {
    "system/*.read".into()
}

/// Bulk submit configuration ($bulk-submit, $bulk-submit-status).
///
/// Download and batch limits are shared with `bulk_import`; manifest and file URLs
/// go through the same outbound policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkSubmitConfig {
    /// Offer the operations. Default: true; with no `submitters` every request is refused.
    #[serde(default = "default_bulk_submit_enabled")]
    pub enabled: bool,

    /// Data Providers this server accepts submissions from.
    #[serde(default)]
    pub submitters: Vec<BulkSubmitter>,
}

fn default_bulk_submit_enabled() -> bool {
    true
}

impl Default for BulkSubmitConfig {
    fn default() -> Self {
        Self {
            enabled: default_bulk_submit_enabled(),
            submitters: Vec::new(),
        }
    }
}

/// Bulk import configuration ($import operation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkImportConfig {
    /// Enable bulk import functionality
    #[serde(default = "default_bulk_import_enabled")]
    pub enabled: bool,

    /// Number of resources to create per transaction batch
    #[serde(default = "default_bulk_import_batch_size")]
    pub batch_size: usize,

    /// Maximum concurrent import jobs
    #[serde(default = "default_bulk_import_max_concurrent")]
    pub max_concurrent_jobs: usize,

    /// Maximum number of resources processed concurrently within a single import job
    #[serde(default = "default_bulk_import_parallelism")]
    pub max_parallel_resources: usize,

    /// Maximum bytes downloaded for each source, including blank lines.
    #[serde(default = "default_bulk_import_source_bytes")]
    pub max_source_bytes: u64,

    /// Maximum NDJSON line length and native transaction input size.
    #[serde(default = "default_bulk_import_line_bytes")]
    pub max_line_bytes: usize,
    #[serde(default = "default_bulk_import_batch_bytes")]
    pub max_batch_bytes: usize,

    /// Bound source descriptors and retained partial failure details.
    #[serde(default = "default_bulk_import_sources")]
    pub max_sources: usize,
    #[serde(default = "default_bulk_import_errors")]
    pub max_errors: usize,

    /// Skip FHIR validation for trusted data (can be overridden per-request)
    #[serde(default)]
    pub default_skip_validation: bool,
}

fn default_bulk_import_enabled() -> bool {
    true
}
fn default_bulk_import_batch_size() -> usize {
    1000
}
fn default_bulk_import_max_concurrent() -> usize {
    3
}
fn default_bulk_import_parallelism() -> usize {
    32
}

fn default_bulk_import_source_bytes() -> u64 {
    1024 * 1024 * 1024
}
fn default_bulk_import_line_bytes() -> usize {
    1024 * 1024
}
fn default_bulk_import_batch_bytes() -> usize {
    16 * 1024 * 1024
}
fn default_bulk_import_sources() -> usize {
    32
}
fn default_bulk_import_errors() -> usize {
    1000
}

impl Default for BulkImportConfig {
    fn default() -> Self {
        Self {
            enabled: default_bulk_import_enabled(),
            batch_size: default_bulk_import_batch_size(),
            max_concurrent_jobs: default_bulk_import_max_concurrent(),
            max_parallel_resources: default_bulk_import_parallelism(),
            max_source_bytes: default_bulk_import_source_bytes(),
            max_line_bytes: default_bulk_import_line_bytes(),
            max_batch_bytes: default_bulk_import_batch_bytes(),
            max_sources: default_bulk_import_sources(),
            max_errors: default_bulk_import_errors(),
            default_skip_validation: false,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BootstrapConfig {
    /// Admin user configuration
    /// If set, creates an admin user on first startup (if not already exists)
    #[serde(default)]
    pub admin_user: Option<AdminUserConfig>,

    /// Backend service client configuration
    /// If set, creates a confidential OAuth client for backend services on first startup
    #[serde(default)]
    pub backend_client: Option<BackendClientConfig>,
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
    /// FHIR user reference (e.g., "Practitioner/admin")
    #[serde(default)]
    pub fhir_user: Option<String>,
}

/// Configuration for bootstrapping a backend service client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendClientConfig {
    /// Backend client ID (default: "octofhir-backend")
    #[serde(default = "default_backend_client_id")]
    pub client_id: String,

    /// Client secret (optional - will be auto-generated if not provided)
    /// For security, prefer using OCTOFHIR__BOOTSTRAP__BACKEND_CLIENT__CLIENT_SECRET env var
    #[serde(default)]
    pub client_secret: Option<String>,

    /// OAuth scopes (default: ["system/*.cruds"])
    #[serde(default = "default_backend_scopes")]
    pub scopes: Vec<String>,

    /// Human-readable name (optional)
    #[serde(default)]
    pub name: Option<String>,

    /// Description (optional)
    #[serde(default)]
    pub description: Option<String>,
}

fn default_backend_client_id() -> String {
    "octofhir-backend".to_string()
}

fn default_backend_scopes() -> Vec<String> {
    vec!["system/*.cruds".to_string()]
}

/// Audit trail configuration
///
/// Controls automatic creation of FHIR AuditEvent resources for
/// tracking system activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Enable audit logging
    /// Default: true
    #[serde(default = "default_audit_enabled")]
    pub enabled: bool,

    /// Log FHIR resource operations (create, read, update, delete, search)
    /// Default: true
    #[serde(default = "default_audit_log_fhir")]
    pub log_fhir_operations: bool,

    /// Log authentication events (login, logout, failed attempts)
    /// Default: true
    #[serde(default = "default_audit_log_auth")]
    pub log_auth_events: bool,

    /// Log read operations (GET requests)
    /// Can be disabled to reduce audit volume
    /// Default: false
    #[serde(default)]
    pub log_read_operations: bool,

    /// Log search operations
    /// Can be disabled to reduce audit volume
    /// Default: false
    #[serde(default)]
    pub log_search_operations: bool,

    /// Resource types to exclude from audit logging
    /// Example: ["AuditEvent"] to avoid infinite loops
    #[serde(default = "default_audit_exclude_types")]
    pub exclude_resource_types: Vec<String>,
}

fn default_audit_enabled() -> bool {
    true
}

fn default_audit_log_fhir() -> bool {
    true
}

fn default_audit_log_auth() -> bool {
    true
}

fn default_audit_exclude_types() -> Vec<String> {
    vec!["AuditEvent".to_string()]
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: default_audit_enabled(),
            log_fhir_operations: default_audit_log_fhir(),
            log_auth_events: default_audit_log_auth(),
            log_read_operations: false,
            log_search_operations: false,
            exclude_resource_types: default_audit_exclude_types(),
        }
    }
}

/// SQL on FHIR configuration for ViewDefinition editor and operations
///
/// Configures the SQL on FHIR feature which enables:
/// - ViewDefinition resource CRUD operations
/// - $run operation to execute ViewDefinitions and get tabular results
/// - $export operation to export results in various formats (CSV, Parquet, NDJSON)
///
/// When enabled, the SQL on FHIR IG package (`hl7.fhir.uv.sql-on-fhir`) is installed,
/// which creates the `viewdefinition` table dynamically via FCM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SqlOnFhirConfig {
    /// Enable SQL on FHIR feature
    /// When true, installs the SQL on FHIR package and enables ViewDefinition operations
    /// Default: false
    pub enabled: bool,
    /// `ViewDefinition/$export`, the export of SQL on FHIR 2.0, which 3.0 replaced with
    /// `$sql-export`. Default: true.
    pub viewdefinition_export: bool,
}

impl Default for SqlOnFhirConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            viewdefinition_export: true,
        }
    }
}

/// FHIR Mapping Language (`StructureMap/$transform`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FmlConfig {
    /// Register `StructureMap/$transform`. Default: true.
    pub enabled: bool,
    /// Role a user needs for `debug=true`: the trace holds every value the map reads and writes,
    /// so it is clinical data. Default: `admin`.
    pub debug_role: String,
    /// A transform running longer is cancelled. Default: 30000.
    pub timeout_ms: u64,
    /// Rules one transform may run. Default: 20 000 000.
    pub max_rules: u64,
    /// Trace events kept with `debug=true`; later ones are left out (`truncated`). Default: 10 000.
    pub max_trace_events: usize,
    /// Folders of FML maps (`.fml`) and StructureMap / ConceptMap JSON, searched after the
    /// server's own resources and its packages: for maps no package publishes, such as the HL7
    /// cross-version maps (a checkout of github.com/HL7/fhir-cross-version, `input`) that
    /// `$convert` uses.
    pub map_folders: Vec<String>,
}

impl Default for FmlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debug_role: "admin".into(),
            timeout_ms: 30_000,
            max_rules: 20_000_000,
            max_trace_events: 10_000,
            map_folders: Vec::new(),
        }
    }
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
        // List values (e.g. OCTOFHIR__PACKAGES__LOAD=a,b,c) are split on `,`
        // for the keys registered below.
        builder = builder.add_source(
            Environment::with_prefix("OCTOFHIR")
                .try_parsing(true)
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("packages.load")
                .with_list_parse_key("search.indexed_params")
                .with_list_parse_key("search.ucum_indexed_params")
                .with_list_parse_key("storage.postgres.document_gin_resource_types"),
        );
        let cfg = builder
            .build()
            .map_err(|e| format!("config build error: {e}"))?;
        let mut merged: AppConfig = cfg
            .try_deserialize()
            .map_err(|e| format!("config deserialize error: {e}"))?;
        // Flat-env composite indexes (the structured `[[search.composite_index]]`
        // TOML form deserializes above; this appends the compact env list on top).
        if let Ok(raw) = std::env::var("OCTOFHIR__SEARCH__COMPOSITE_INDEXES") {
            for entry in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match super::CompositeIndexSpec::parse_compact(entry) {
                    Some(spec) if !merged.search.composite_index.contains(&spec) => {
                        merged.search.composite_index.push(spec);
                    }
                    Some(_) => {}
                    None => {
                        return Err(format!(
                            "invalid OCTOFHIR__SEARCH__COMPOSITE_INDEXES entry '{entry}' \
                         (want ResourceType.param=system|code)"
                        ));
                    }
                }
            }
        }
        // Make source paths independent of the launch directory and preserve this
        // interpretation on reload. Programmatically constructed configs use cwd.
        let config_file = PathBuf::from(path.unwrap_or("octofhir.toml"));
        let absolute_file = std::path::absolute(&config_file)
            .map_err(|e| format!("cannot resolve configuration path: {e}"))?;
        let base = absolute_file
            .parent()
            .ok_or("configuration has no parent directory")?;
        for spec in &mut merged.packages.load {
            if let super::PackageSpec::Table {
                path: Some(source), ..
            } = spec
                && Path::new(source).is_relative()
            {
                *source = base.join(&*source).to_string_lossy().into_owned();
            }
        }
        if let Some(cache) = &mut merged.packages.path {
            if Path::new(cache).is_relative() {
                *cache = base.join(&*cache).to_string_lossy().into_owned();
            }
        } else {
            merged.packages.path = Some(base.join(".fhir").to_string_lossy().into_owned());
        }
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
