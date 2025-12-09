//! Bootstrap module for loading internal conformance resources and admin users.
//!
//! This module loads StructureDefinitions, ValueSets, and CodeSystems from
//! embedded resources into the database on first startup.
//!
//! It also handles admin user and default UI client creation from configuration
//! or environment variables.
//!
//! Resources are embedded at compile time using `include_str!` for single-binary distribution.

use octofhir_auth::policy::resources::{
    AccessPolicy, EngineElement, MatcherElement, PolicyEngineType,
};
use octofhir_auth::storage::{ClientStorage, PolicyStorage, User, UserStorage};
use octofhir_auth::types::{Client, GrantType};
use octofhir_db_postgres::PostgresConformanceStorage;
use octofhir_storage::ConformanceStorage;
use tracing::{info, warn};

use crate::config::AdminUserConfig;

/// Default UI client ID - hardcoded for the built-in admin UI
pub const DEFAULT_UI_CLIENT_ID: &str = "octofhir-ui";

/// Default UI client name
pub const DEFAULT_UI_CLIENT_NAME: &str = "OctoFHIR Admin UI";

/// Default admin access policy ID (fixed UUID for idempotent bootstrap)
pub const ADMIN_ACCESS_POLICY_ID: &str = "00000000-0000-0000-0000-000000000001";

/// Default admin access policy name
pub const ADMIN_ACCESS_POLICY_NAME: &str = "Admin Full Access";

/// Embedded internal IG resources
/// These are compiled into the binary for single-binary distribution
const EMBEDDED_RESOURCES: &[(&str, &str)] = &[
    // StructureDefinitions - Gateway
    (
        "StructureDefinition-App.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-App.json"),
    ),
    (
        "StructureDefinition-CustomOperation.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-CustomOperation.json"),
    ),
    // StructureDefinitions - Auth
    (
        "StructureDefinition-Client.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-Client.json"),
    ),
    (
        "StructureDefinition-User.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-User.json"),
    ),
    (
        "StructureDefinition-AccessPolicy.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-AccessPolicy.json"),
    ),
    (
        "StructureDefinition-Session.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-Session.json"),
    ),
    (
        "StructureDefinition-RefreshToken.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-RefreshToken.json"),
    ),
    (
        "StructureDefinition-RevokedToken.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-RevokedToken.json"),
    ),
    (
        "StructureDefinition-IdentityProvider.json",
        include_str!("../../../igs/octofhir-internal/StructureDefinition-IdentityProvider.json"),
    ),
    // ValueSets
    (
        "ValueSet-http-methods.json",
        include_str!("../../../igs/octofhir-internal/ValueSet-http-methods.json"),
    ),
    (
        "ValueSet-operation-types.json",
        include_str!("../../../igs/octofhir-internal/ValueSet-operation-types.json"),
    ),
    (
        "ValueSet-identity-provider-types.json",
        include_str!("../../../igs/octofhir-internal/ValueSet-identity-provider-types.json"),
    ),
    // CodeSystems
    (
        "CodeSystem-http-methods.json",
        include_str!("../../../igs/octofhir-internal/CodeSystem-http-methods.json"),
    ),
    (
        "CodeSystem-operation-types.json",
        include_str!("../../../igs/octofhir-internal/CodeSystem-operation-types.json"),
    ),
    (
        "CodeSystem-identity-provider-types.json",
        include_str!("../../../igs/octofhir-internal/CodeSystem-identity-provider-types.json"),
    ),
];

/// Bootstraps conformance resources from embedded resources into the database.
///
/// This function:
/// 1. Checks if resources already exist (idempotent)
/// 2. Loads embedded JSON resources (compiled into binary)
/// 3. Inserts StructureDefinitions, ValueSets, and CodeSystems
///
/// # Errors
///
/// Returns an error if:
/// - JSON files are malformed
/// - Database operations fail
pub async fn bootstrap_conformance_resources(
    conformance_storage: &PostgresConformanceStorage,
) -> Result<BootstrapStats, Box<dyn std::error::Error>> {
    info!("Starting conformance resource bootstrap from embedded resources");

    let mut stats = BootstrapStats::default();

    // Check if already bootstrapped (check for App StructureDefinition)
    if let Ok(Some(_)) = conformance_storage
        .get_structure_definition_by_url("http://octofhir.io/StructureDefinition/App", None)
        .await
    {
        info!("Conformance resources already bootstrapped, skipping");
        return Ok(stats);
    }

    info!(
        "Loading {} embedded conformance resources",
        EMBEDDED_RESOURCES.len()
    );

    // Load all embedded resources
    for (filename, content) in EMBEDDED_RESOURCES {
        // Parse the resource
        let resource: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse {}: {}", filename, e))?;

        let resource_type = resource["resourceType"]
            .as_str()
            .ok_or_else(|| format!("Missing resourceType in {}", filename))?;

        let name = resource["name"].as_str().unwrap_or("unknown");

        // Insert based on resource type
        match resource_type {
            "StructureDefinition" => {
                conformance_storage
                    .create_structure_definition(&resource)
                    .await?;
                info!("Loaded StructureDefinition: {}", name);
                stats.structure_definitions += 1;
            }
            "ValueSet" => {
                conformance_storage.create_value_set(&resource).await?;
                info!("Loaded ValueSet: {}", name);
                stats.value_sets += 1;
            }
            "CodeSystem" => {
                conformance_storage.create_code_system(&resource).await?;
                info!("Loaded CodeSystem: {}", name);
                stats.code_systems += 1;
            }
            "SearchParameter" => {
                conformance_storage
                    .create_search_parameter(&resource)
                    .await?;
                info!("Loaded SearchParameter: {}", name);
                stats.search_parameters += 1;
            }
            other => {
                warn!("Skipping unsupported resource type: {}", other);
            }
        }
    }

    info!(
        structure_definitions = stats.structure_definitions,
        value_sets = stats.value_sets,
        code_systems = stats.code_systems,
        search_parameters = stats.search_parameters,
        total = stats.total(),
        "Conformance bootstrap completed"
    );

    Ok(stats)
}

/// Statistics about the bootstrap operation.
#[derive(Debug, Default)]
pub struct BootstrapStats {
    pub structure_definitions: usize,
    pub value_sets: usize,
    pub code_systems: usize,
    pub search_parameters: usize,
}

impl BootstrapStats {
    /// Returns the total number of resources loaded.
    pub fn total(&self) -> usize {
        self.structure_definitions + self.value_sets + self.code_systems + self.search_parameters
    }
}

/// Bootstraps admin user from configuration.
///
/// Creates an admin user with the "admin" role if:
/// 1. Admin user config is provided (via config file or env vars)
/// 2. A user with the same username doesn't already exist
///
/// # Arguments
///
/// * `user_storage` - Storage backend for user operations
/// * `admin_config` - Admin user configuration (username, password, email)
///
/// # Returns
///
/// Returns `true` if an admin user was created, `false` if user already exists.
///
/// # Errors
///
/// Returns an error if:
/// - Password hashing fails
/// - Database operations fail
///
/// # Example Config (octofhir.toml)
///
/// ```toml
/// [bootstrap.admin_user]
/// username = "admin"
/// password = "your-secure-password"
/// email = "admin@example.com"
/// ```
///
/// # Environment Variables
///
/// ```bash
/// OCTOFHIR__BOOTSTRAP__ADMIN_USER__USERNAME=admin
/// OCTOFHIR__BOOTSTRAP__ADMIN_USER__PASSWORD=your-secure-password
/// OCTOFHIR__BOOTSTRAP__ADMIN_USER__EMAIL=admin@example.com
/// ```
pub async fn bootstrap_admin_user<S: UserStorage>(
    user_storage: &S,
    admin_config: &AdminUserConfig,
) -> Result<bool, Box<dyn std::error::Error>> {
    info!(
        username = %admin_config.username,
        "Checking if admin user needs to be bootstrapped"
    );

    // Check if user already exists
    if let Some(existing) = user_storage
        .find_by_username(&admin_config.username)
        .await?
    {
        info!(
            user_id = %existing.id,
            username = %existing.username,
            "Admin user already exists, skipping bootstrap"
        );
        return Ok(false);
    }

    // Hash the password
    let password_hash = hash_password(&admin_config.password)?;

    // Create the admin user
    let mut user = User::builder(&admin_config.username)
        .password_hash(&password_hash)
        .add_role("admin")
        .active(true)
        .build();

    // Add email if provided
    if let Some(ref email) = admin_config.email {
        user = User::builder(&admin_config.username)
            .password_hash(&password_hash)
            .email(email)
            .add_role("admin")
            .active(true)
            .build();
    }

    user_storage.create(&user).await?;

    info!(
        user_id = %user.id,
        username = %user.username,
        email = ?admin_config.email,
        "Admin user created successfully"
    );

    Ok(true)
}

/// Hash a password using bcrypt.
fn hash_password(password: &str) -> Result<String, Box<dyn std::error::Error>> {
    use bcrypt::{DEFAULT_COST, hash};
    let hashed = hash(password, DEFAULT_COST)?;
    Ok(hashed)
}

/// Bootstraps the default UI client for the admin interface.
///
/// Creates a public OAuth client that the built-in UI uses for authentication.
/// This client is configured for the authorization code flow with PKCE.
///
/// # Arguments
///
/// * `client_storage` - Storage backend for client operations
/// * `issuer` - The server's issuer URL (used for redirect URIs)
///
/// # Returns
///
/// Returns `true` if a client was created, `false` if it already exists.
pub async fn bootstrap_default_ui_client<S: ClientStorage>(
    client_storage: &S,
    issuer: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    info!(
        client_id = DEFAULT_UI_CLIENT_ID,
        "Checking if default UI client needs to be bootstrapped"
    );

    // Check if client already exists
    if let Some(_existing) = client_storage
        .find_by_client_id(DEFAULT_UI_CLIENT_ID)
        .await?
    {
        info!(
            client_id = DEFAULT_UI_CLIENT_ID,
            "Default UI client already exists, skipping bootstrap"
        );
        return Ok(false);
    }

    // Build redirect URIs based on the issuer
    let redirect_uris = vec![
        format!("{}/ui/", issuer),
        format!("{}/ui/callback", issuer),
        // Also allow localhost for development
        "http://localhost:5173/".to_string(),
        "http://localhost:5173/callback".to_string(),
    ];

    // Create the default UI client (public client for browser-based apps)
    let client = Client {
        client_id: DEFAULT_UI_CLIENT_ID.to_string(),
        client_secret: None, // Public client - no secret
        name: DEFAULT_UI_CLIENT_NAME.to_string(),
        description: Some("Built-in OAuth client for the OctoFHIR Admin UI".to_string()),
        grant_types: vec![
            GrantType::AuthorizationCode,
            GrantType::RefreshToken,
            GrantType::Password,
        ],
        redirect_uris,
        scopes: vec![
            "openid".to_string(),
            "offline_access".to_string(),
            "user/*.cruds".to_string(),
            "system/*.cruds".to_string(),
        ],
        confidential: false, // Public client for browser-based UI
        active: true,
        access_token_lifetime: None,
        refresh_token_lifetime: None,
        pkce_required: Some(true), // PKCE required for public clients
        allowed_origins: vec![issuer.to_string(), "http://localhost:5173".to_string()],
        jwks: None,
        jwks_uri: None,
    };

    client_storage.create(&client).await?;

    info!(
        client_id = DEFAULT_UI_CLIENT_ID,
        client_name = DEFAULT_UI_CLIENT_NAME,
        "Default UI client created successfully"
    );

    Ok(true)
}

/// Bootstraps the admin access policy for the admin interface.
///
/// Creates an access policy that allows all operations for admin users
/// when using the octofhir-ui client. This policy is required for the
/// admin UI to function properly.
///
/// # Arguments
///
/// * `policy_storage` - Storage backend for policy operations
///
/// # Returns
///
/// Returns `true` if a policy was created, `false` if it already exists.
pub async fn bootstrap_admin_access_policy<S: PolicyStorage>(
    policy_storage: &S,
) -> Result<bool, Box<dyn std::error::Error>> {
    info!(
        policy_id = ADMIN_ACCESS_POLICY_ID,
        "Checking if admin access policy needs to be bootstrapped"
    );

    // Check if policy already exists
    if let Some(_existing) = policy_storage.get(ADMIN_ACCESS_POLICY_ID).await? {
        info!(
            policy_id = ADMIN_ACCESS_POLICY_ID,
            "Admin access policy already exists, skipping bootstrap"
        );
        return Ok(false);
    }

    // Create the admin access policy
    let policy = AccessPolicy {
        id: Some(ADMIN_ACCESS_POLICY_ID.to_string()),
        name: ADMIN_ACCESS_POLICY_NAME.to_string(),
        description: Some(
            "Allow all operations for admin users via octofhir-ui client".to_string(),
        ),
        active: true,
        priority: 1, // High priority (evaluated first)
        matcher: Some(MatcherElement {
            clients: Some(vec![DEFAULT_UI_CLIENT_ID.to_string()]),
            roles: Some(vec!["admin".to_string()]),
            ..Default::default()
        }),
        engine: EngineElement {
            engine_type: PolicyEngineType::Allow,
            script: None,
        },
        ..Default::default()
    };

    policy_storage.create(&policy).await?;

    info!(
        policy_id = ADMIN_ACCESS_POLICY_ID,
        policy_name = ADMIN_ACCESS_POLICY_NAME,
        "Admin access policy created successfully"
    );

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_stats_total() {
        let stats = BootstrapStats {
            structure_definitions: 2,
            value_sets: 2,
            code_systems: 2,
            search_parameters: 0,
        };

        assert_eq!(stats.total(), 6);
    }
}
