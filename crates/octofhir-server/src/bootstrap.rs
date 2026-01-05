//! Bootstrap module for loading internal conformance resources and admin users.
//!
//! This module loads StructureDefinitions, ValueSets, and CodeSystems from
//! embedded resources into the database on first startup.
//!
//! It also handles admin user and default UI client creation from configuration
//! or environment variables.
//!
//! Resources are embedded at compile time using `include_str!` for single-binary distribution.

use std::sync::Arc;

use octofhir_auth::policy::resources::{
    AccessPolicy, EngineElement, MatcherElement, PolicyEngineType,
};
use octofhir_auth::storage::{ClientStorage, PolicyStorage, User, UserStorage};
use octofhir_auth::types::{Client, GrantType};
use octofhir_core::OperationProvider;
use sqlx_postgres::PgPool;
use tracing::info;

use crate::config::AdminUserConfig;
use crate::config::AppConfig;
use crate::operation_registry::{
    AuthOperationProvider, FhirOperationProvider, NotificationsOperationProvider,
    OperationRegistryService, PostgresOperationStorage, SystemOperationProvider,
    UiOperationProvider,
};

/// Default UI client ID - hardcoded for the built-in admin UI
pub const DEFAULT_UI_CLIENT_ID: &str = "octofhir-ui";

/// Default UI client name
pub const DEFAULT_UI_CLIENT_NAME: &str = "OctoFHIR Admin UI";

/// Default admin access policy ID (fixed UUID for idempotent bootstrap)
pub const ADMIN_ACCESS_POLICY_ID: &str = "00000000-0000-0000-0000-000000000001";

/// Default admin access policy name
pub const ADMIN_ACCESS_POLICY_NAME: &str = "Admin Full Access";

/// Default backend client ID
pub const DEFAULT_BACKEND_CLIENT_ID: &str = "octofhir-backend";

/// Default backend client name
pub const DEFAULT_BACKEND_CLIENT_NAME: &str = "OctoFHIR Backend Service";

/// Default backend access policy ID (fixed UUID for idempotent bootstrap)
pub const BACKEND_ACCESS_POLICY_ID: &str = "00000000-0000-0000-0000-000000000002";

/// Default backend access policy name
pub const BACKEND_ACCESS_POLICY_NAME: &str = "Backend Full Access";

/// Embedded octofhir-auth IG resources
/// Authentication and authorization resources
pub const EMBEDDED_AUTH_RESOURCES: &[(&str, &str)] = &[
    (
        "StructureDefinition-User.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-User.json"),
    ),
    (
        "StructureDefinition-Session.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-Session.json"),
    ),
    (
        "StructureDefinition-Client.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-Client.json"),
    ),
    (
        "StructureDefinition-AccessPolicy.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-AccessPolicy.json"),
    ),
    (
        "StructureDefinition-Role.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-Role.json"),
    ),
    (
        "StructureDefinition-RefreshToken.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-RefreshToken.json"),
    ),
    (
        "StructureDefinition-RevokedToken.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-RevokedToken.json"),
    ),
    (
        "StructureDefinition-IdentityProvider.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-IdentityProvider.json"),
    ),
    (
        "CodeSystem-identity-provider-types.json",
        include_str!("../../../igs/octofhir-auth/CodeSystem-identity-provider-types.json"),
    ),
    (
        "ValueSet-identity-provider-types.json",
        include_str!("../../../igs/octofhir-auth/ValueSet-identity-provider-types.json"),
    ),
    // AuthSession (SSO session management)
    (
        "StructureDefinition-AuthSession.json",
        include_str!("../../../igs/octofhir-auth/StructureDefinition-AuthSession.json"),
    ),
    (
        "CodeSystem-session-status.json",
        include_str!("../../../igs/octofhir-auth/CodeSystem-session-status.json"),
    ),
    (
        "ValueSet-session-status.json",
        include_str!("../../../igs/octofhir-auth/ValueSet-session-status.json"),
    ),
    (
        "SearchParameter-AuthSession-subject.json",
        include_str!("../../../igs/octofhir-auth/SearchParameter-AuthSession-subject.json"),
    ),
    (
        "SearchParameter-AuthSession-status.json",
        include_str!("../../../igs/octofhir-auth/SearchParameter-AuthSession-status.json"),
    ),
    (
        "SearchParameter-AuthSession-expires-at.json",
        include_str!("../../../igs/octofhir-auth/SearchParameter-AuthSession-expires-at.json"),
    ),
    (
        "SearchParameter-AuthSession-session-token.json",
        include_str!("../../../igs/octofhir-auth/SearchParameter-AuthSession-session-token.json"),
    ),
    // Client search parameters
    (
        "SearchParameter-Client-clientId.json",
        include_str!("../../../igs/octofhir-auth/SearchParameter-Client-clientId.json"),
    ),
];

/// Embedded octofhir-app IG resources
/// Application-level resources (operations, apps)
pub const EMBEDDED_APP_RESOURCES: &[(&str, &str)] = &[
    (
        "StructureDefinition-App.json",
        include_str!("../../../igs/octofhir-app/StructureDefinition-App.json"),
    ),
    (
        "StructureDefinition-CustomOperation.json",
        include_str!("../../../igs/octofhir-app/StructureDefinition-CustomOperation.json"),
    ),
    (
        "StructureDefinition-AppSubscription.json",
        include_str!("../../../igs/octofhir-app/StructureDefinition-AppSubscription.json"),
    ),
    (
        "ValueSet-http-methods.json",
        include_str!("../../../igs/octofhir-app/ValueSet-http-methods.json"),
    ),
    (
        "ValueSet-operation-types.json",
        include_str!("../../../igs/octofhir-app/ValueSet-operation-types.json"),
    ),
    (
        "ValueSet-operation-outcome-type.json",
        include_str!("../../../igs/octofhir-app/ValueSet-operation-outcome-type.json"),
    ),
    (
        "ValueSet-app-status.json",
        include_str!("../../../igs/octofhir-app/ValueSet-app-status.json"),
    ),
    (
        "CodeSystem-http-methods.json",
        include_str!("../../../igs/octofhir-app/CodeSystem-http-methods.json"),
    ),
    (
        "CodeSystem-operation-types.json",
        include_str!("../../../igs/octofhir-app/CodeSystem-operation-types.json"),
    ),
    (
        "CodeSystem-operation-outcome-type.json",
        include_str!("../../../igs/octofhir-app/CodeSystem-operation-outcome-type.json"),
    ),
    (
        "CodeSystem-app-status.json",
        include_str!("../../../igs/octofhir-app/CodeSystem-app-status.json"),
    ),
];

/// Embedded SQL on FHIR compatibility resources
/// Backports abstract types from R4B/R5 needed for SQL on FHIR ViewDefinition support
pub const EMBEDDED_SOF_COMPAT_RESOURCES: &[(&str, &str)] = &[(
    "StructureDefinition-CanonicalResource.json",
    include_str!("../../../igs/octofhir-auth/StructureDefinition-CanonicalResource.json"),
)];

/// Embedded octofhir-notifications IG resources
/// Notification provider, template, and log resources
pub const EMBEDDED_NOTIFICATIONS_RESOURCES: &[(&str, &str)] = &[
    (
        "StructureDefinition-NotificationProvider.json",
        include_str!("../../../igs/octofhir-notifications/StructureDefinition-NotificationProvider.json"),
    ),
    (
        "StructureDefinition-NotificationTemplate.json",
        include_str!("../../../igs/octofhir-notifications/StructureDefinition-NotificationTemplate.json"),
    ),
    (
        "StructureDefinition-NotificationLog.json",
        include_str!("../../../igs/octofhir-notifications/StructureDefinition-NotificationLog.json"),
    ),
    (
        "CodeSystem-notification-provider-types.json",
        include_str!("../../../igs/octofhir-notifications/CodeSystem-notification-provider-types.json"),
    ),
    (
        "CodeSystem-notification-status.json",
        include_str!("../../../igs/octofhir-notifications/CodeSystem-notification-status.json"),
    ),
    (
        "ValueSet-notification-provider-types.json",
        include_str!("../../../igs/octofhir-notifications/ValueSet-notification-provider-types.json"),
    ),
    (
        "ValueSet-notification-status.json",
        include_str!("../../../igs/octofhir-notifications/ValueSet-notification-status.json"),
    ),
];

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

    // Create the admin user with builder pattern
    let mut builder = User::builder(&admin_config.username)
        .password_hash(&password_hash)
        .add_role("admin")
        .active(true);

    // Add email if provided
    if let Some(ref email) = admin_config.email {
        builder = builder.email(email);
    }

    // Add FHIR user reference if provided
    if let Some(ref fhir_user) = admin_config.fhir_user {
        builder = builder.fhir_user(fhir_user);
    }

    let user = builder.build();

    user_storage.create(&user).await?;

    info!(
        user_id = %user.id,
        username = %user.username,
        email = ?admin_config.email,
        "Admin user created successfully"
    );

    Ok(true)
}

/// Hash a password using Argon2id.
///
/// This is a thin wrapper around `octofhir_auth::hash_password` for backwards compatibility.
pub fn hash_password(password: &str) -> Result<String, Box<dyn std::error::Error>> {
    octofhir_auth::hash_password(password).map_err(|e| e.into())
}

/// Generate a secure random client secret (256-bit, hex-encoded to 64 chars).
pub fn generate_client_secret() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    hex::encode(bytes)
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
        post_logout_redirect_uris: vec![
            format!("{}/ui/", issuer),
            "http://localhost:5173/".to_string(),
        ],
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

/// Bootstraps the backend service client for machine-to-machine authentication.
///
/// Creates a confidential OAuth client configured for the client_credentials grant type.
/// This client is intended for backend services, automation scripts, and server-to-server
/// communication.
///
/// # Secret Management
///
/// - If `client_secret` is provided in config/env, it will be used and hashed
/// - If `client_secret` is NOT provided, a secure random secret will be auto-generated
/// - Auto-generated secrets are logged ONCE on creation (never on subsequent startups)
/// - Plaintext secrets are NEVER persisted to disk
///
/// # Arguments
///
/// * `client_storage` - Storage backend for client operations
/// * `backend_config` - Backend client configuration (client_id, scopes, etc.)
///
/// # Returns
///
/// Returns `(created: bool, plaintext_secret: Option<String>)` where:
/// - `created` is `true` if a new client was created, `false` if it already exists
/// - `plaintext_secret` is `Some(secret)` if a new secret was auto-generated, `None` otherwise
///
/// # Errors
///
/// Returns an error if:
/// - Secret hashing fails
/// - Database operations fail
/// - Client validation fails
pub async fn bootstrap_backend_client<S: ClientStorage>(
    client_storage: &S,
    backend_config: &crate::config::BackendClientConfig,
) -> Result<(bool, Option<String>), Box<dyn std::error::Error>> {
    info!(
        client_id = %backend_config.client_id,
        "Checking if backend service client needs to be bootstrapped"
    );

    // Check if client already exists
    if let Some(_existing) = client_storage
        .find_by_client_id(&backend_config.client_id)
        .await?
    {
        info!(
            client_id = %backend_config.client_id,
            "Backend service client already exists, skipping bootstrap"
        );
        return Ok((false, None));
    }

    // Determine secret: use configured or generate new
    let (plaintext_secret, hashed_secret, secret_was_generated) =
        if let Some(ref configured_secret) = backend_config.client_secret {
            let hash = hash_password(configured_secret)?;
            (configured_secret.clone(), hash, false)
        } else {
            let plain = generate_client_secret();
            let hash = hash_password(&plain)?;
            (plain, hash, true)
        };

    let name = backend_config
        .name
        .as_deref()
        .unwrap_or(DEFAULT_BACKEND_CLIENT_NAME);

    let description = backend_config.description.as_deref().unwrap_or(
        "Backend service client for machine-to-machine authentication via client_credentials grant",
    );

    // Create confidential client for client_credentials grant
    let client = Client {
        client_id: backend_config.client_id.clone(),
        client_secret: Some(hashed_secret),
        name: name.to_string(),
        description: Some(description.to_string()),
        grant_types: vec![GrantType::ClientCredentials],
        redirect_uris: vec![],
        post_logout_redirect_uris: vec![], // Backend clients don't need logout redirects
        scopes: backend_config.scopes.clone(),
        confidential: true,
        active: true,
        access_token_lifetime: None,
        refresh_token_lifetime: None,
        pkce_required: None,
        allowed_origins: vec![],
        jwks: None,
        jwks_uri: None,
    };

    client
        .validate()
        .map_err(|e| format!("Backend client validation failed: {}", e))?;

    client_storage.create(&client).await?;

    info!(
        client_id = %backend_config.client_id,
        scopes = ?backend_config.scopes,
        secret_auto_generated = secret_was_generated,
        "Backend service client created successfully"
    );

    Ok((
        true,
        if secret_was_generated {
            Some(plaintext_secret)
        } else {
            None
        },
    ))
}

/// Bootstraps the admin access policy for the admin interface.
///
/// Creates or updates an access policy that allows all operations for admin users
/// when using the octofhir-ui client. This policy is required for the
/// admin UI to function properly.
///
/// The policy includes:
/// - FHIR operations (fhir.*)
/// - GraphQL operations (graphql.*)
/// - System operations (system.*)
/// - UI operations (ui.*)
/// - Auth operations (auth.*)
///
/// # Arguments
///
/// * `policy_storage` - Storage backend for policy operations
///
/// # Returns
///
/// Returns `true` if a policy was created or updated.
pub async fn bootstrap_admin_access_policy<S: PolicyStorage>(
    policy_storage: &S,
) -> Result<bool, Box<dyn std::error::Error>> {
    info!(
        policy_id = ADMIN_ACCESS_POLICY_ID,
        "Syncing admin access policy"
    );

    // Create the admin access policy with all operation categories
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

    // Always upsert to ensure policy stays in sync
    policy_storage.upsert(&policy).await?;

    info!(
        policy_id = ADMIN_ACCESS_POLICY_ID,
        policy_name = ADMIN_ACCESS_POLICY_NAME,
        "Admin access policy synced successfully"
    );

    Ok(true)
}

/// Bootstraps the backend access policy for machine-to-machine authentication.
///
/// Creates or updates an access policy that allows all operations for the backend
/// service client. This policy is required for backend services using the
/// client_credentials grant to access the FHIR API.
///
/// The policy includes:
/// - FHIR operations (fhir.*)
/// - GraphQL operations (graphql.*)
/// - System operations (system.*)
/// - UI operations (ui.*)
/// - Auth operations (auth.*)
///
/// # Arguments
///
/// * `policy_storage` - Storage backend for policy operations
/// * `backend_client_id` - The client_id of the backend service client
///
/// # Returns
///
/// Returns `true` if a policy was created or updated.
pub async fn bootstrap_backend_access_policy<S: PolicyStorage>(
    policy_storage: &S,
    backend_client_id: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    info!(
        policy_id = BACKEND_ACCESS_POLICY_ID,
        backend_client_id = %backend_client_id,
        "Syncing backend access policy"
    );

    // Create the backend access policy with all operation categories
    let policy = AccessPolicy {
        id: Some(BACKEND_ACCESS_POLICY_ID.to_string()),
        name: BACKEND_ACCESS_POLICY_NAME.to_string(),
        description: Some(format!(
            "Allow all operations for backend service client '{}'",
            backend_client_id
        )),
        active: true,
        priority: 1, // High priority (evaluated first)
        matcher: Some(MatcherElement {
            clients: Some(vec![backend_client_id.to_string()]),
            ..Default::default()
        }),
        engine: EngineElement {
            engine_type: PolicyEngineType::Allow,
            script: None,
        },
        ..Default::default()
    };

    // Always upsert to ensure policy stays in sync
    policy_storage.upsert(&policy).await?;

    info!(
        policy_id = BACKEND_ACCESS_POLICY_ID,
        policy_name = BACKEND_ACCESS_POLICY_NAME,
        backend_client_id = %backend_client_id,
        "Backend access policy synced successfully"
    );

    Ok(true)
}

/// Bootstraps the operation registry with all operation providers.
///
/// Collects operations from all enabled modules and syncs them to the database.
/// This ensures all operations are registered for UI display and policy targeting.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `config` - Application configuration to check which modules are enabled
///
/// # Returns
///
/// Returns the initialized OperationRegistryService with synced operations.
pub async fn bootstrap_operations(
    pool: &PgPool,
    config: &AppConfig,
) -> Result<Arc<OperationRegistryService>, Box<dyn std::error::Error>> {
    info!("Bootstrapping operations registry");

    // Create storage adapter
    let op_storage = Arc::new(PostgresOperationStorage::new(pool.clone()));

    // Collect operation providers based on enabled modules
    let mut providers: Vec<Arc<dyn OperationProvider>> = vec![
        // FHIR operations (always enabled)
        Arc::new(FhirOperationProvider),
        // System operations (always enabled)
        Arc::new(SystemOperationProvider),
        // UI operations (always enabled)
        Arc::new(UiOperationProvider),
    ];

    // Add GraphQL provider if enabled
    if config.graphql.enabled {
        use octofhir_graphql::GraphQLOperationProvider;
        providers.push(Arc::new(GraphQLOperationProvider));
    }

    // Add Auth provider (always enabled)
    providers.push(Arc::new(AuthOperationProvider));

    // Add Notifications provider
    providers.push(Arc::new(NotificationsOperationProvider));

    // Note: Gateway CustomOperations are NOT stored in the operations table
    // to avoid duplication. They are loaded dynamically by the /api/operations endpoint.

    // Create registry service
    let registry = Arc::new(OperationRegistryService::with_providers(op_storage, providers));

    // Sync operations to database (also rebuilds in-memory indexes)
    let count = registry
        .sync_operations(true)
        .await
        .map_err(|e| format!("Failed to sync operations: {}", e))?;

    info!(count, "Operations synced to database");
    Ok(registry)
}
