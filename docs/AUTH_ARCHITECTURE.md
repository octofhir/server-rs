# OctoFHIR Auth Module Architecture

## Executive Summary

This document presents a comprehensive architectural plan for the OctoFHIR Auth module—a production-grade authentication, authorization, and access control system designed for a modern FHIR R4/R5 server in Rust. The architecture supports:

- **Built-in OAuth 2.0 Server** with authorization code, client credentials, and refresh token flows
- **External Identity Provider Federation** via OpenID Connect
- **Full SMART on FHIR Compliance** (STU 2.2) including App Launch and Backend Services
- **Custom FHIR Resources** for auth configuration via internal Implementation Guide
- **AccessPolicy Engine** with pattern matching and scriptable policies (Rhai + QuickJS)

---

## Table of Contents

1. [Design Principles](#1-design-principles)
2. [High-Level Architecture](#2-high-level-architecture)
3. [Component Deep Dives](#3-component-deep-dives)
   - 3.1 [OAuth 2.0 Authorization Server](#31-oauth-20-authorization-server)
   - 3.2 [Token Service](#32-token-service)
   - 3.3 [Identity Provider Federation](#33-identity-provider-federation)
   - 3.4 [SMART on FHIR Module](#34-smart-on-fhir-module)
   - 3.5 [AccessPolicy Engine](#35-accesspolicy-engine)
   - 3.6 [Audit Service](#36-audit-service)
4. [Custom FHIR Resources (IG)](#4-custom-fhir-resources-ig)
5. [Data Models](#5-data-models)
6. [API Endpoints](#6-api-endpoints)
7. [Security Considerations](#7-security-considerations)
8. [Integration Points](#8-integration-points)
9. [Configuration](#9-configuration)
10. [Implementation Roadmap](#10-implementation-roadmap)

---

## 1. Design Principles

### 1.1 Core Tenets

| Principle | Description |
|-----------|-------------|
| **Zero Trust** | Every request is authenticated and authorized; no implicit trust |
| **Defense in Depth** | Multiple security layers (transport, token, policy, audit) |
| **Standards Compliance** | Full FHIR R4/R5, OAuth 2.0, OpenID Connect, SMART on FHIR STU 2.2 |
| **Extensibility** | Plugin architecture for custom policies and identity providers |
| **Performance** | Sub-millisecond policy evaluation; async-first design |
| **Persistence** | No in-memory-only state; all auth data in PostgreSQL |

### 1.2 Architectural Constraints

- **No In-Memory Storage**: All session state, tokens, and policies stored in PostgreSQL
- **Horizontal Scalability**: Stateless request handling; shared database state
- **Hot Reload**: Policy and configuration changes without server restart
- **Audit Everything**: Security-relevant events logged as FHIR AuditEvent resources
- **Alpine Linux Compatible**: All components must work with musl libc

---

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              OctoFHIR Server                                │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   Axum      │  │  Content    │  │   Auth      │  │    FHIR Resource    │ │
│  │  Router     │──│ Negotiation │──│ Middleware  │──│     Handlers        │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│         │                                │                     │            │
│         ▼                                ▼                     ▼            │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                         Auth Module (octofhir-auth)                     ││
│  ├─────────────────────────────────────────────────────────────────────────┤│
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐               ││
│  │  │ OAuth 2.0     │  │  SMART on     │  │   Identity    │               ││
│  │  │ Server        │  │  FHIR         │  │   Federation  │               ││
│  │  │               │  │               │  │               │               ││
│  │  │ • /authorize  │  │ • Scopes      │  │ • OIDC Client │               ││
│  │  │ • /token      │  │ • Launch Ctx  │  │ • JWKS Cache  │               ││
│  │  │ • /revoke     │  │ • Discovery   │  │ • User Sync   │               ││
│  │  └───────┬───────┘  └───────┬───────┘  └───────┬───────┘               ││
│  │          │                  │                  │                        ││
│  │          ▼                  ▼                  ▼                        ││
│  │  ┌─────────────────────────────────────────────────────────────────┐   ││
│  │  │                     Token Service                                │   ││
│  │  │  • JWT Generation (RS256/RS384/ES384)                           │   ││
│  │  │  • Token Validation & Introspection                             │   ││
│  │  │  • Refresh Token Management                                      │   ││
│  │  │  • PKCE Verification                                             │   ││
│  │  └─────────────────────────┬───────────────────────────────────────┘   ││
│  │                            │                                            ││
│  │                            ▼                                            ││
│  │  ┌─────────────────────────────────────────────────────────────────┐   ││
│  │  │                   AccessPolicy Engine                            │   ││
│  │  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │   ││
│  │  │  │  Pattern    │  │  Rhai       │  │  QuickJS (rquickjs)     │  │   ││
│  │  │  │  Matcher    │  │  Scripting  │  │  JavaScript Runtime     │  │   ││
│  │  │  └─────────────┘  └─────────────┘  └─────────────────────────┘  │   ││
│  │  └─────────────────────────┬───────────────────────────────────────┘   ││
│  │                            │                                            ││
│  │                            ▼                                            ││
│  │  ┌─────────────────────────────────────────────────────────────────┐   ││
│  │  │                     Audit Service                                │   ││
│  │  │  • AuditEvent Generation (FHIR R4)                              │   ││
│  │  │  • Security Event Logging                                        │   ││
│  │  │  • Compliance Reporting                                          │   ││
│  │  └─────────────────────────────────────────────────────────────────┘   ││
│  └─────────────────────────────────────────────────────────────────────────┘│
│                                      │                                      │
│                                      ▼                                      │
│  ┌─────────────────────────────────────────────────────────────────────────┐│
│  │                     PostgreSQL (octofhir_auth Schema)                   ││
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           ││
│  │  │ Client  │ │ Session │ │  Token  │ │ Access  │ │  Audit  │           ││
│  │  │         │ │         │ │         │ │ Policy  │ │  Event  │           ││
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘           ││
│  └─────────────────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Crate Structure

```
crates/
├── octofhir-auth/                    # Core auth crate
│   ├── src/
│   │   ├── lib.rs                    # Public API exports
│   │   ├── oauth/                    # OAuth 2.0 implementation
│   │   │   ├── mod.rs
│   │   │   ├── server.rs             # Authorization server
│   │   │   ├── flows.rs              # Auth code, client creds, refresh
│   │   │   ├── pkce.rs               # PKCE implementation
│   │   │   └── errors.rs
│   │   ├── token/                    # Token management
│   │   │   ├── mod.rs
│   │   │   ├── jwt.rs                # JWT generation/validation
│   │   │   ├── introspection.rs      # RFC 7662
│   │   │   ├── revocation.rs         # RFC 7009
│   │   │   └── refresh.rs
│   │   ├── smart/                    # SMART on FHIR
│   │   │   ├── mod.rs
│   │   │   ├── scopes.rs             # Scope parsing/validation
│   │   │   ├── launch.rs             # Launch context
│   │   │   ├── discovery.rs          # .well-known/smart-configuration
│   │   │   └── conformance.rs        # CapabilityStatement extensions
│   │   ├── federation/               # External IdP support
│   │   │   ├── mod.rs
│   │   │   ├── oidc.rs               # OpenID Connect client
│   │   │   ├── jwks.rs               # JWKS fetching/caching
│   │   │   └── providers.rs          # Provider registry
│   │   ├── policy/                   # AccessPolicy engine
│   │   │   ├── mod.rs
│   │   │   ├── engine.rs             # Policy evaluation orchestrator
│   │   │   ├── matcher.rs            # Pattern matching
│   │   │   ├── rhai.rs               # Rhai script runtime
│   │   │   ├── quickjs.rs            # QuickJS JavaScript runtime
│   │   │   └── context.rs            # Evaluation context
│   │   ├── middleware/               # Axum middleware
│   │   │   ├── mod.rs
│   │   │   ├── auth.rs               # Authentication extractor
│   │   │   └── authorize.rs          # Authorization enforcement
│   │   ├── audit/                    # Audit logging
│   │   │   ├── mod.rs
│   │   │   └── events.rs             # AuditEvent generation
│   │   ├── storage/                  # Persistence traits
│   │   │   ├── mod.rs
│   │   │   └── traits.rs
│   │   └── types.rs                  # Shared types
│   └── Cargo.toml
│
├── octofhir-auth-postgres/           # PostgreSQL auth storage
│   ├── src/
│   │   ├── lib.rs
│   │   ├── client.rs                 # Client storage
│   │   ├── session.rs                # Session storage
│   │   ├── token.rs                  # Token storage
│   │   ├── policy.rs                 # AccessPolicy storage
│   │   └── migrations/
│   └── Cargo.toml
```

---

## 3. Component Deep Dives

### 3.1 OAuth 2.0 Authorization Server

#### 3.1.1 Supported Flows

| Flow | RFC | Use Case |
|------|-----|----------|
| **Authorization Code + PKCE** | RFC 6749, RFC 7636 | User-facing apps, EHR launch |
| **Client Credentials** | RFC 6749 | Backend services, system-to-system |
| **Refresh Token** | RFC 6749 | Token renewal without re-auth |

#### 3.1.2 Authorization Code Flow

```rust
// crates/octofhir-auth/src/oauth/flows.rs

pub struct AuthorizationRequest {
    pub response_type: ResponseType,        // Must be "code"
    pub client_id: ClientId,
    pub redirect_uri: Url,
    pub scope: SmartScopes,
    pub state: String,                      // Min 122 bits entropy
    pub code_challenge: String,             // S256 PKCE
    pub code_challenge_method: CodeChallengeMethod, // Must be S256
    pub aud: Url,                           // FHIR server base URL
    pub launch: Option<String>,             // EHR launch context
}

pub struct AuthorizationResponse {
    pub code: AuthorizationCode,            // One-time use
    pub state: String,                      // Echo back
}

pub struct TokenRequest {
    pub grant_type: GrantType,              // "authorization_code"
    pub code: AuthorizationCode,
    pub redirect_uri: Url,
    pub code_verifier: String,              // PKCE verifier
    pub client_id: Option<ClientId>,        // Required for public clients
    // For confidential clients: client_assertion or HTTP Basic Auth
}

pub struct TokenResponse {
    pub access_token: AccessToken,
    pub token_type: TokenType,              // "Bearer"
    pub expires_in: u64,                    // Seconds (recommended: 300-3600)
    pub scope: SmartScopes,                 // May differ from request
    pub refresh_token: Option<RefreshToken>,
    pub id_token: Option<IdToken>,          // If openid scope requested
    // SMART launch context
    pub patient: Option<String>,
    pub encounter: Option<String>,
    pub fhir_context: Option<Vec<FhirContext>>,
    pub need_patient_banner: Option<bool>,
    pub smart_style_url: Option<Url>,
}
```

#### 3.1.3 Client Credentials Flow (Backend Services)

```rust
pub struct ClientCredentialsRequest {
    pub grant_type: GrantType,              // "client_credentials"
    pub client_assertion_type: String,      // "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    pub client_assertion: String,           // Signed JWT
    pub scope: SmartScopes,                 // system/* scopes only
}

// JWT Assertion Claims (RFC 7523)
pub struct ClientAssertionClaims {
    pub iss: ClientId,                      // Client ID
    pub sub: ClientId,                      // Client ID (same as iss)
    pub aud: Url,                           // Token endpoint URL
    pub exp: i64,                           // Max 5 minutes from now
    pub jti: String,                        // Unique identifier
    pub iat: Option<i64>,                   // Issued at
}
```

#### 3.1.4 PKCE Implementation

```rust
// crates/octofhir-auth/src/oauth/pkce.rs

use sha2::{Sha256, Digest};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub struct PkceVerifier(String);
pub struct PkceChallenge(String);

impl PkceChallenge {
    /// Generate S256 challenge from verifier
    pub fn from_verifier(verifier: &PkceVerifier) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(verifier.0.as_bytes());
        let hash = hasher.finalize();
        Self(URL_SAFE_NO_PAD.encode(hash))
    }

    /// Verify that a verifier matches this challenge
    pub fn verify(&self, verifier: &PkceVerifier) -> bool {
        Self::from_verifier(verifier).0 == self.0
    }
}

pub enum CodeChallengeMethod {
    S256,  // Only supported method; "plain" is explicitly forbidden
}
```

### 3.2 Token Service

#### 3.2.1 JWT Structure

```rust
// crates/octofhir-auth/src/token/jwt.rs

use jsonwebtoken::{Algorithm, EncodingKey, DecodingKey, Header, Validation};

/// Supported signing algorithms
pub enum SigningAlgorithm {
    RS256,  // RSA with SHA-256
    RS384,  // RSA with SHA-384 (SMART preferred)
    ES384,  // ECDSA with P-384 (SMART preferred)
}

/// Access token claims
#[derive(Debug, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    // Standard JWT claims
    pub iss: String,                        // Issuer (OctoFHIR server URL)
    pub sub: String,                        // Subject (user or client ID)
    pub aud: Vec<String>,                   // Audience (FHIR server URLs)
    pub exp: i64,                           // Expiration time
    pub iat: i64,                           // Issued at
    pub jti: String,                        // JWT ID (for revocation)

    // SMART-specific claims
    pub scope: String,                      // Space-separated scopes
    pub client_id: String,                  // OAuth client ID

    // Context claims (optional)
    pub patient: Option<String>,            // Patient context
    pub encounter: Option<String>,          // Encounter context
    pub fhir_user: Option<String>,          // User's FHIR resource
}

/// ID token claims (OpenID Connect)
#[derive(Debug, Serialize, Deserialize)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub nonce: Option<String>,
    pub fhir_user: Option<String>,          // FHIR resource URL
    pub profile: Option<String>,            // Profile URL
}

/// Token validation result
pub struct ValidatedToken {
    pub claims: AccessTokenClaims,
    pub client: Client,
    pub user: Option<User>,
    pub scopes: SmartScopes,
}
```

#### 3.2.2 Token Introspection (RFC 7662)

```rust
// crates/octofhir-auth/src/token/introspection.rs

#[derive(Debug, Serialize)]
pub struct IntrospectionResponse {
    pub active: bool,

    // Only present if active=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    // SMART context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,
}
```

#### 3.2.3 Token Revocation (RFC 7009)

```rust
// crates/octofhir-auth/src/token/revocation.rs

pub struct RevocationRequest {
    pub token: String,
    pub token_type_hint: Option<TokenTypeHint>,
}

pub enum TokenTypeHint {
    AccessToken,
    RefreshToken,
}

/// Revocation storage tracks revoked token JTIs
#[async_trait]
pub trait RevocationStorage: Send + Sync {
    async fn revoke(&self, jti: &str, exp: i64) -> Result<(), AuthError>;
    async fn is_revoked(&self, jti: &str) -> Result<bool, AuthError>;
    async fn cleanup_expired(&self) -> Result<u64, AuthError>;
}
```

### 3.3 Identity Provider Federation

#### 3.3.1 OpenID Connect Client

```rust
// crates/octofhir-auth/src/federation/oidc.rs

pub struct OidcProvider {
    pub id: ProviderId,
    pub name: String,
    pub issuer: Url,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub discovery_url: Url,
    pub scopes: Vec<String>,
    pub user_mapping: UserMappingConfig,
}

pub struct OidcDiscoveryDocument {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub userinfo_endpoint: Option<Url>,
    pub jwks_uri: Url,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
}

pub struct UserMappingConfig {
    /// Claim to use as user identifier
    pub subject_claim: String,              // Default: "sub"
    /// Claim to use as email
    pub email_claim: Option<String>,        // Default: "email"
    /// Claim to use for roles
    pub roles_claim: Option<String>,
    /// FHIR resource type for user
    pub fhir_resource_type: FhirUserType,   // Practitioner, Patient, etc.
    /// Auto-create user on first login
    pub auto_provision: bool,
}

pub enum FhirUserType {
    Practitioner,
    Patient,
    RelatedPerson,
    Person,
}
```

#### 3.3.2 JWKS Caching

```rust
// crates/octofhir-auth/src/federation/jwks.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use jsonwebtoken::jwk::JwkSet;

pub struct JwksCache {
    cache: Arc<RwLock<HashMap<Url, CachedJwks>>>,
    http_client: reqwest::Client,
    default_ttl: Duration,
}

struct CachedJwks {
    jwks: JwkSet,
    fetched_at: Instant,
    expires_at: Instant,
}

impl JwksCache {
    pub async fn get_key(
        &self,
        jwks_uri: &Url,
        kid: &str,
    ) -> Result<DecodingKey, JwksError> {
        // Check cache first
        if let Some(key) = self.get_cached_key(jwks_uri, kid).await {
            return Ok(key);
        }

        // Fetch and cache
        self.refresh(jwks_uri).await?;

        self.get_cached_key(jwks_uri, kid)
            .await
            .ok_or(JwksError::KeyNotFound(kid.to_string()))
    }

    pub async fn refresh(&self, jwks_uri: &Url) -> Result<(), JwksError> {
        let response = self.http_client
            .get(jwks_uri.as_str())
            .send()
            .await?;

        let jwks: JwkSet = response.json().await?;

        let mut cache = self.cache.write().await;
        cache.insert(jwks_uri.clone(), CachedJwks {
            jwks,
            fetched_at: Instant::now(),
            expires_at: Instant::now() + self.default_ttl,
        });

        Ok(())
    }
}
```

### 3.4 SMART on FHIR Module

#### 3.4.1 Scope Parsing and Validation

```rust
// crates/octofhir-auth/src/smart/scopes.rs

use std::collections::HashSet;

/// SMART v2 scope format: context/ResourceType.cruds?param=value
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartScope {
    pub context: ScopeContext,
    pub resource_type: ResourceType,
    pub permissions: Permissions,
    pub filter: Option<ScopeFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeContext {
    Patient,    // patient/*
    User,       // user/*
    System,     // system/* (backend services only)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    Specific(String),  // e.g., "Observation"
    Wildcard,          // "*"
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Permissions {
    pub create: bool,   // 'c'
    pub read: bool,     // 'r'
    pub update: bool,   // 'u'
    pub delete: bool,   // 'd'
    pub search: bool,   // 's'
}

impl Permissions {
    pub fn from_str(s: &str) -> Result<Self, ScopeError> {
        let mut perms = Self::default();
        let mut last_char = None;

        for c in s.chars() {
            // Validate ordering: c < r < u < d < s
            if let Some(prev) = last_char {
                if c <= prev {
                    return Err(ScopeError::InvalidPermissionOrder);
                }
            }

            match c {
                'c' => perms.create = true,
                'r' => perms.read = true,
                'u' => perms.update = true,
                'd' => perms.delete = true,
                's' => perms.search = true,
                _ => return Err(ScopeError::InvalidPermission(c)),
            }
            last_char = Some(c);
        }

        Ok(perms)
    }
}

/// Optional filter on scope (e.g., ?category=laboratory)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFilter {
    pub parameter: String,
    pub value: String,
}

/// Collection of parsed scopes
#[derive(Debug, Clone, Default)]
pub struct SmartScopes {
    pub resource_scopes: Vec<SmartScope>,
    pub launch: bool,
    pub launch_patient: bool,
    pub launch_encounter: bool,
    pub openid: bool,
    pub fhir_user: bool,
    pub offline_access: bool,
    pub online_access: bool,
}

impl SmartScopes {
    pub fn parse(scope_string: &str) -> Result<Self, ScopeError> {
        let mut scopes = Self::default();

        for scope in scope_string.split_whitespace() {
            match scope {
                "launch" => scopes.launch = true,
                "launch/patient" => scopes.launch_patient = true,
                "launch/encounter" => scopes.launch_encounter = true,
                "openid" => scopes.openid = true,
                "fhirUser" => scopes.fhir_user = true,
                "offline_access" => scopes.offline_access = true,
                "online_access" => scopes.online_access = true,
                s => {
                    if let Ok(resource_scope) = Self::parse_resource_scope(s) {
                        scopes.resource_scopes.push(resource_scope);
                    }
                    // Unknown scopes are ignored per spec
                }
            }
        }

        Ok(scopes)
    }

    /// Check if scopes permit an operation
    pub fn permits(
        &self,
        resource_type: &str,
        operation: FhirOperation,
        patient_context: Option<&str>,
    ) -> bool {
        for scope in &self.resource_scopes {
            if scope.matches(resource_type, operation, patient_context) {
                return true;
            }
        }
        false
    }
}
```

#### 3.4.2 Launch Context

```rust
// crates/octofhir-auth/src/smart/launch.rs

/// EHR launch context received from authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchContext {
    /// Opaque launch parameter from EHR
    pub launch_id: String,
    /// Patient context
    pub patient: Option<String>,
    /// Encounter context
    pub encounter: Option<String>,
    /// Additional FHIR resource context
    pub fhir_context: Vec<FhirContextItem>,
    /// Display patient banner
    pub need_patient_banner: bool,
    /// EHR styling URL
    pub smart_style_url: Option<Url>,
    /// App intent
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirContextItem {
    pub reference: String,
    pub role: Option<String>,
}

/// Launch context storage
#[async_trait]
pub trait LaunchContextStorage: Send + Sync {
    /// Store launch context (short TTL ~10 minutes)
    async fn store(&self, launch_id: &str, context: &LaunchContext) -> Result<(), AuthError>;

    /// Retrieve and consume launch context
    async fn consume(&self, launch_id: &str) -> Result<Option<LaunchContext>, AuthError>;
}
```

#### 3.4.3 Discovery Endpoint

```rust
// crates/octofhir-auth/src/smart/discovery.rs

/// SMART configuration document
/// Served at /.well-known/smart-configuration
#[derive(Debug, Serialize)]
pub struct SmartConfiguration {
    // Required
    pub issuer: Option<String>,             // Required if sso-openid-connect
    pub authorization_endpoint: Option<String>, // Required if launch-*
    pub token_endpoint: String,
    pub grant_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>, // Must include "S256"
    pub capabilities: Vec<String>,

    // Recommended
    pub jwks_uri: Option<String>,
    pub scopes_supported: Option<Vec<String>>,
    pub response_types_supported: Option<Vec<String>>,

    // Optional
    pub management_endpoint: Option<String>,
    pub introspection_endpoint: Option<String>,
    pub revocation_endpoint: Option<String>,
    pub registration_endpoint: Option<String>,
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    pub user_access_brand_bundle: Option<String>,
    pub user_access_brand_identifier: Option<String>,
}

impl SmartConfiguration {
    pub fn build(config: &AuthConfig, base_url: &Url) -> Self {
        let mut capabilities = vec![
            "permission-v2".to_string(),
        ];

        if config.smart.launch_ehr_enabled {
            capabilities.push("launch-ehr".to_string());
            capabilities.push("context-ehr-patient".to_string());
            capabilities.push("context-ehr-encounter".to_string());
        }

        if config.smart.launch_standalone_enabled {
            capabilities.push("launch-standalone".to_string());
            capabilities.push("context-standalone-patient".to_string());
        }

        if config.smart.public_clients_allowed {
            capabilities.push("client-public".to_string());
        }

        if config.smart.confidential_symmetric_allowed {
            capabilities.push("client-confidential-symmetric".to_string());
        }

        if config.smart.confidential_asymmetric_allowed {
            capabilities.push("client-confidential-asymmetric".to_string());
        }

        if config.smart.refresh_tokens_enabled {
            capabilities.push("permission-offline".to_string());
            capabilities.push("permission-online".to_string());
        }

        if config.smart.openid_enabled {
            capabilities.push("sso-openid-connect".to_string());
        }

        capabilities.push("permission-patient".to_string());
        capabilities.push("permission-user".to_string());

        Self {
            issuer: config.smart.openid_enabled.then(|| base_url.to_string()),
            authorization_endpoint: Some(format!("{}/auth/authorize", base_url)),
            token_endpoint: format!("{}/auth/token", base_url),
            grant_types_supported: vec![
                "authorization_code".to_string(),
                "client_credentials".to_string(),
                "refresh_token".to_string(),
            ],
            code_challenge_methods_supported: vec!["S256".to_string()],
            capabilities,
            jwks_uri: Some(format!("{}/.well-known/jwks.json", base_url)),
            scopes_supported: Some(config.smart.supported_scopes.clone()),
            response_types_supported: Some(vec!["code".to_string()]),
            management_endpoint: None,
            introspection_endpoint: Some(format!("{}/auth/introspect", base_url)),
            revocation_endpoint: Some(format!("{}/auth/revoke", base_url)),
            registration_endpoint: config.smart.dynamic_registration_enabled
                .then(|| format!("{}/auth/register", base_url)),
            token_endpoint_auth_methods_supported: Some(vec![
                "client_secret_basic".to_string(),
                "client_secret_post".to_string(),
                "private_key_jwt".to_string(),
            ]),
            user_access_brand_bundle: None,
            user_access_brand_identifier: None,
        }
    }
}
```

### 3.5 AccessPolicy Engine

#### 3.5.1 Policy Evaluation Architecture

```rust
// crates/octofhir-auth/src/policy/engine.rs

use async_trait::async_trait;

/// Access decision result
#[derive(Debug, Clone)]
pub enum AccessDecision {
    /// Access granted
    Allow,
    /// Access denied with reason
    Deny(DenyReason),
    /// Cannot make decision, continue to next policy
    Abstain,
}

#[derive(Debug, Clone)]
pub struct DenyReason {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

/// Context for policy evaluation
#[derive(Debug, Clone)]
pub struct PolicyContext {
    // Identity
    pub user: Option<UserIdentity>,
    pub client: ClientIdentity,
    pub scopes: SmartScopes,

    // Request
    pub operation: FhirOperation,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub request_body: Option<serde_json::Value>,
    pub query_params: HashMap<String, String>,

    // Context
    pub patient_context: Option<String>,
    pub encounter_context: Option<String>,

    // Resource (for update/delete)
    pub existing_resource: Option<serde_json::Value>,

    // Environment
    pub request_time: DateTime<Utc>,
    pub source_ip: Option<IpAddr>,
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub id: String,
    pub fhir_user: Option<String>,          // e.g., "Practitioner/123"
    pub roles: Vec<String>,
    pub attributes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ClientIdentity {
    pub id: String,
    pub name: String,
    pub trusted: bool,
}

/// Policy evaluation engine
pub struct PolicyEngine {
    pattern_matcher: PatternMatcher,
    rhai_runtime: Option<RhaiRuntime>,
    quickjs_runtime: Option<QuickJsRuntimePool>,
    policy_storage: Arc<dyn PolicyStorage>,
}

impl PolicyEngine {
    pub async fn evaluate(&self, context: &PolicyContext) -> AccessDecision {
        // Load applicable policies
        let policies = self.policy_storage
            .find_applicable(&context.resource_type, &context.operation)
            .await;

        // Evaluate in order (first deny wins, then first allow)
        let mut has_allow = false;

        for policy in policies {
            match self.evaluate_policy(&policy, context).await {
                AccessDecision::Deny(reason) => {
                    return AccessDecision::Deny(reason);
                }
                AccessDecision::Allow => {
                    has_allow = true;
                }
                AccessDecision::Abstain => continue,
            }
        }

        if has_allow {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny(DenyReason {
                code: "no-matching-policy".to_string(),
                message: "No policy granted access".to_string(),
                details: None,
            })
        }
    }

    async fn evaluate_policy(
        &self,
        policy: &AccessPolicy,
        context: &PolicyContext,
    ) -> AccessDecision {
        // Check matchers first
        if !self.pattern_matcher.matches(&policy.matchers, context) {
            return AccessDecision::Abstain;
        }

        // Evaluate policy logic
        match &policy.engine {
            PolicyEngineType::Allow => AccessDecision::Allow,
            PolicyEngineType::Deny => AccessDecision::Deny(DenyReason {
                code: "policy-denied".to_string(),
                message: policy.deny_message.clone().unwrap_or_default(),
                details: None,
            }),
            PolicyEngineType::Rhai(script) => {
                self.evaluate_rhai(script, context).await
            }
            PolicyEngineType::QuickJs(script) => {
                self.evaluate_quickjs(script, context).await
            }
        }
    }
}
```

#### 3.5.2 Pattern Matcher

```rust
// crates/octofhir-auth/src/policy/matcher.rs

/// Matchers determine which requests a policy applies to
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyMatchers {
    /// Match by client ID patterns
    pub clients: Option<Vec<MatchPattern>>,
    /// Match by user roles
    pub roles: Option<Vec<String>>,
    /// Match by user FHIR resource type
    pub user_types: Option<Vec<String>>,  // Practitioner, Patient, etc.
    /// Match by FHIR resource type
    pub resource_types: Option<Vec<String>>,
    /// Match by FHIR operation
    pub operations: Option<Vec<FhirOperation>>,
    /// Match by compartment
    pub compartments: Option<Vec<CompartmentMatcher>>,
    /// Match by request path pattern
    pub paths: Option<Vec<String>>,
    /// Match by source IP CIDR
    pub source_ips: Option<Vec<IpNetwork>>,
    /// Custom FHIRPath expression
    pub fhirpath: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchPattern {
    Exact(String),
    Prefix(String),
    Suffix(String),
    Regex(String),
    Wildcard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMatcher {
    pub compartment_type: String,           // "Patient", "Practitioner", etc.
    pub compartment_id: CompartmentIdSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompartmentIdSource {
    /// From SMART launch context
    LaunchContext,
    /// From user's FHIR resource
    UserFhirResource,
    /// Fixed value
    Fixed(String),
    /// From request parameter
    RequestParam(String),
}

pub struct PatternMatcher {
    regex_cache: HashMap<String, Regex>,
}

impl PatternMatcher {
    pub fn matches(&self, matchers: &PolicyMatchers, context: &PolicyContext) -> bool {
        // All specified matchers must match (AND logic)

        if let Some(clients) = &matchers.clients {
            if !self.match_any_pattern(clients, &context.client.id) {
                return false;
            }
        }

        if let Some(roles) = &matchers.roles {
            if let Some(user) = &context.user {
                if !roles.iter().any(|r| user.roles.contains(r)) {
                    return false;
                }
            } else {
                return false;
            }
        }

        if let Some(resource_types) = &matchers.resource_types {
            if !resource_types.contains(&context.resource_type)
                && !resource_types.contains(&"*".to_string()) {
                return false;
            }
        }

        if let Some(operations) = &matchers.operations {
            if !operations.contains(&context.operation) {
                return false;
            }
        }

        // ... additional matcher evaluations

        true
    }
}
```

#### 3.5.3 Rhai Script Runtime (Lightweight)

```rust
// crates/octofhir-auth/src/policy/rhai.rs

use rhai::{Engine, AST, Scope, Dynamic, EvalAltResult};

/// Lightweight Rust-native scripting for simple policies
pub struct RhaiRuntime {
    engine: Engine,
    script_cache: HashMap<String, AST>,
}

impl RhaiRuntime {
    pub fn new() -> Self {
        let mut engine = Engine::new();

        // Sandbox configuration
        engine.set_max_operations(10_000);        // Prevent runaway scripts
        engine.set_max_call_levels(32);           // Limit recursion
        engine.set_max_expr_depths(64, 64);       // Limit expression depth
        engine.set_max_string_size(10_000);       // Limit string size
        engine.set_max_array_size(1_000);         // Limit array size
        engine.set_max_map_size(1_000);           // Limit map size

        // Disable dangerous features
        engine.disable_symbol("eval");            // No dynamic code execution

        // Register custom functions
        engine.register_fn("has_role", |user: &mut Dynamic, role: &str| -> bool {
            user.clone_cast::<rhai::Map>()
                .get("roles")
                .and_then(|r| r.clone().try_cast::<rhai::Array>())
                .map(|roles| roles.iter().any(|r| {
                    r.clone().into_string().ok() == Some(role.to_string())
                }))
                .unwrap_or(false)
        });

        engine.register_fn("in_compartment", |context: &mut Dynamic, compartment_type: &str, compartment_id: &str| -> bool {
            // Check if resource is in specified compartment
            true // Placeholder
        });

        Self {
            engine,
            script_cache: HashMap::new(),
        }
    }

    pub fn evaluate(
        &self,
        script: &str,
        context: &PolicyContext,
    ) -> AccessDecision {
        // Build scope with context variables
        let mut scope = Scope::new();

        scope.push("user", self.context_to_dynamic(&context.user));
        scope.push("client", self.client_to_dynamic(&context.client));
        scope.push("request", self.request_to_dynamic(context));
        scope.push("resource", context.existing_resource.clone().unwrap_or(serde_json::Value::Null));
        scope.push("patient_context", context.patient_context.clone().unwrap_or_default());

        // Evaluate script
        match self.engine.eval_with_scope::<bool>(&mut scope, script) {
            Ok(true) => AccessDecision::Allow,
            Ok(false) => AccessDecision::Deny(DenyReason {
                code: "script-denied".to_string(),
                message: "Access denied by policy script".to_string(),
                details: None,
            }),
            Err(e) => {
                tracing::error!(error = %e, "Rhai script evaluation error");
                AccessDecision::Deny(DenyReason {
                    code: "script-error".to_string(),
                    message: "Policy evaluation error".to_string(),
                    details: Some(serde_json::json!({ "error": e.to_string() })),
                })
            }
        }
    }
}
```

#### 3.5.4 QuickJS Runtime (Full JavaScript ES2020)

```rust
// crates/octofhir-auth/src/policy/quickjs.rs

use rquickjs::{Runtime, Context, Function, Value};
use std::time::{Duration, Instant};
use std::sync::Arc;
use parking_lot::Mutex;

/// Configuration for QuickJS runtime
pub struct QuickJsConfig {
    /// Memory limit in bytes (e.g., 16MB)
    pub memory_limit: usize,
    /// Maximum stack size in bytes
    pub max_stack_size: usize,
    /// Execution timeout
    pub timeout: Duration,
    /// Number of runtimes in pool (for parallelism)
    pub pool_size: usize,
}

impl Default for QuickJsConfig {
    fn default() -> Self {
        Self {
            memory_limit: 16 * 1024 * 1024,  // 16MB
            max_stack_size: 256 * 1024,       // 256KB
            timeout: Duration::from_millis(100),
            pool_size: num_cpus::get(),
        }
    }
}

/// Pool of QuickJS runtimes for parallel execution
/// Each runtime is independent - safe for multithreading
pub struct QuickJsRuntimePool {
    runtimes: Vec<Arc<Mutex<QuickJsInstance>>>,
    config: QuickJsConfig,
}

struct QuickJsInstance {
    runtime: Runtime,
}

impl QuickJsRuntimePool {
    pub fn new(config: QuickJsConfig) -> Result<Self, PolicyError> {
        let mut runtimes = Vec::with_capacity(config.pool_size);

        for _ in 0..config.pool_size {
            let runtime = Runtime::new()?;

            // Set memory limit
            runtime.set_memory_limit(config.memory_limit);

            // Set stack limit
            runtime.set_max_stack_size(config.max_stack_size);

            runtimes.push(Arc::new(Mutex::new(QuickJsInstance { runtime })));
        }

        Ok(Self { runtimes, config })
    }

    /// Evaluate a policy script with timeout and resource limits
    pub fn evaluate(
        &self,
        script: &str,
        context: &PolicyContext,
    ) -> AccessDecision {
        // Get a runtime from the pool (round-robin or least-loaded)
        let instance = self.get_runtime();
        let mut guard = instance.lock();

        // Set up interrupt handler for timeout
        let start = Instant::now();
        let timeout = self.config.timeout;
        guard.runtime.set_interrupt_handler(Some(Box::new(move || {
            start.elapsed() > timeout
        })));

        // Serialize context to JSON
        let context_json = match serde_json::to_string(context) {
            Ok(json) => json,
            Err(e) => {
                return AccessDecision::Deny(DenyReason {
                    code: "context-serialization-error".to_string(),
                    message: format!("Failed to serialize context: {}", e),
                    details: None,
                });
            }
        };

        // Create execution context and evaluate
        let result = guard.runtime.context(|ctx| {
            // Set up global helper functions
            Self::setup_globals(&ctx)?;

            // Inject context
            let globals = ctx.globals();
            let parsed_ctx: Value = ctx.json_parse(&context_json)?;
            globals.set("ctx", parsed_ctx)?;

            // Wrap and evaluate user script
            let wrapped_script = format!(r#"
                (function() {{
                    const user = ctx.user;
                    const client = ctx.client;
                    const scopes = ctx.scopes;
                    const request = {{
                        operation: ctx.operation,
                        resourceType: ctx.resource_type,
                        resourceId: ctx.resource_id,
                        body: ctx.request_body,
                        params: ctx.query_params,
                    }};
                    const resource = ctx.existing_resource;
                    const patient = ctx.patient_context;
                    const encounter = ctx.encounter_context;

                    // Helper functions
                    const allow = () => ({{ decision: "allow" }});
                    const deny = (reason) => ({{ decision: "deny", reason: reason || "Access denied" }});
                    const abstain = () => ({{ decision: "abstain" }});

                    // User's policy script
                    {script}
                }})()
            "#);

            ctx.eval::<Value, _>(&wrapped_script)
        });

        // Clear interrupt handler
        guard.runtime.set_interrupt_handler(None);

        // Parse result
        match result {
            Ok(value) => Self::parse_decision(value),
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("interrupted") {
                    AccessDecision::Deny(DenyReason {
                        code: "script-timeout".to_string(),
                        message: "Policy script execution timeout".to_string(),
                        details: None,
                    })
                } else {
                    tracing::error!(error = %e, "QuickJS script evaluation error");
                    AccessDecision::Deny(DenyReason {
                        code: "script-error".to_string(),
                        message: "Policy evaluation error".to_string(),
                        details: Some(serde_json::json!({ "error": error_str })),
                    })
                }
            }
        }
    }

    fn setup_globals(ctx: &Context) -> Result<(), rquickjs::Error> {
        // Register helper functions available in all policies
        let globals = ctx.globals();

        // hasRole helper
        globals.set("hasRole", Function::new(ctx.clone(), |user: Value, role: String| {
            // Implementation
            false
        })?)?;

        // inCompartment helper
        globals.set("inCompartment", Function::new(ctx.clone(), |resource: Value, compartment_type: String, compartment_id: String| {
            // Implementation
            false
        })?)?;

        Ok(())
    }

    fn parse_decision(value: Value) -> AccessDecision {
        // Parse the {decision: "allow"|"deny"|"abstain", reason?: string} object
        if let Ok(obj) = value.as_object() {
            if let Some(decision) = obj.get::<_, String>("decision").ok() {
                match decision.as_str() {
                    "allow" => return AccessDecision::Allow,
                    "deny" => {
                        let reason = obj.get::<_, String>("reason").ok()
                            .unwrap_or_else(|| "Access denied".to_string());
                        return AccessDecision::Deny(DenyReason {
                            code: "script-denied".to_string(),
                            message: reason,
                            details: None,
                        });
                    }
                    "abstain" => return AccessDecision::Abstain,
                    _ => {}
                }
            }
        }

        // If script returns boolean directly
        if let Ok(b) = value.as_bool() {
            if b {
                return AccessDecision::Allow;
            } else {
                return AccessDecision::Deny(DenyReason {
                    code: "script-denied".to_string(),
                    message: "Access denied by policy".to_string(),
                    details: None,
                });
            }
        }

        AccessDecision::Abstain
    }

    fn get_runtime(&self) -> Arc<Mutex<QuickJsInstance>> {
        // Simple round-robin; could be improved with load balancing
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % self.runtimes.len();
        self.runtimes[idx].clone()
    }
}

// Example policy scripts (JavaScript ES2020):
//
// // Simple: Allow practitioners to read any Observation
// if (user?.fhirUser?.startsWith("Practitioner/") &&
//     request.operation === "read" &&
//     request.resourceType === "Observation") {
//     return allow();
// }
// return abstain();
//
// // Complex: Patients can only access their own data
// const patientId = user?.fhirUser?.split("/")[1];
// if (user?.fhirUser?.startsWith("Patient/")) {
//     if (patient === patientId ||
//         resource?.subject?.reference === `Patient/${patientId}`) {
//         return allow();
//     }
//     return deny("Patients can only access their own data");
// }
// return abstain();
```

### 3.6 Audit Service

```rust
// crates/octofhir-auth/src/audit/events.rs

use time::OffsetDateTime;

/// Generate FHIR AuditEvent resources for security events
pub struct AuditService {
    storage: Arc<dyn FhirStorage>,
    server_id: String,
}

impl AuditService {
    /// Log authentication event
    pub async fn log_auth_event(&self, event: AuthEvent) -> Result<(), AuditError> {
        let audit_event = self.build_audit_event(&event);
        self.storage.create("AuditEvent", audit_event).await?;
        Ok(())
    }

    fn build_audit_event(&self, event: &AuthEvent) -> serde_json::Value {
        serde_json::json!({
            "resourceType": "AuditEvent",
            "type": {
                "system": "http://terminology.hl7.org/CodeSystem/audit-event-type",
                "code": event.event_type.code(),
                "display": event.event_type.display()
            },
            "subtype": event.subtype.map(|s| [{
                "system": "http://hl7.org/fhir/restful-interaction",
                "code": s.code(),
                "display": s.display()
            }]),
            "action": event.action.code(),
            "period": {
                "start": event.timestamp.format(&time::format_description::well_known::Rfc3339).unwrap()
            },
            "recorded": OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap(),
            "outcome": event.outcome.code(),
            "outcomeDesc": event.outcome_description,
            "agent": self.build_agents(&event.agents),
            "source": {
                "observer": {
                    "identifier": {
                        "value": &self.server_id
                    }
                },
                "type": [{
                    "system": "http://terminology.hl7.org/CodeSystem/security-source-type",
                    "code": "4",
                    "display": "Application Server"
                }]
            },
            "entity": self.build_entities(&event.entities)
        })
    }
}

#[derive(Debug)]
pub struct AuthEvent {
    pub event_type: AuditEventType,
    pub subtype: Option<AuditEventSubtype>,
    pub action: AuditAction,
    pub timestamp: OffsetDateTime,
    pub outcome: AuditOutcome,
    pub outcome_description: Option<String>,
    pub agents: Vec<AuditAgent>,
    pub entities: Vec<AuditEntity>,
}

#[derive(Debug)]
pub enum AuditEventType {
    UserAuthentication,
    Login,
    Logout,
    Export,
    Query,
    AccessDenied,
    PolicyEvaluation,
}

#[derive(Debug)]
pub enum AuditOutcome {
    Success,        // 0
    MinorFailure,   // 4
    SeriousFailure, // 8
    MajorFailure,   // 12
}
```

---

## 4. Custom FHIR Resources (IG)

### 4.1 New Auth Resources for OctoFHIR Internal IG

#### 4.1.1 Client Resource

```json
{
  "resourceType": "StructureDefinition",
  "id": "Client",
  "url": "http://octofhir.io/StructureDefinition/Client",
  "name": "Client",
  "title": "OAuth Client Registration",
  "status": "active",
  "kind": "resource",
  "abstract": false,
  "type": "Client",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/DomainResource",
  "derivation": "specialization",
  "differential": {
    "element": [
      {
        "id": "Client.clientId",
        "path": "Client.clientId",
        "min": 1,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "Client.clientSecret",
        "path": "Client.clientSecret",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }],
        "comment": "Hashed; only for confidential-symmetric clients"
      },
      {
        "id": "Client.name",
        "path": "Client.name",
        "min": 1,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "Client.type",
        "path": "Client.type",
        "min": 1,
        "max": "1",
        "type": [{ "code": "code" }],
        "binding": {
          "strength": "required",
          "valueSet": "http://octofhir.io/ValueSet/client-types"
        },
        "comment": "public | confidential-symmetric | confidential-asymmetric"
      },
      {
        "id": "Client.redirectUris",
        "path": "Client.redirectUris",
        "min": 0,
        "max": "*",
        "type": [{ "code": "uri" }]
      },
      {
        "id": "Client.grantTypes",
        "path": "Client.grantTypes",
        "min": 1,
        "max": "*",
        "type": [{ "code": "code" }],
        "binding": {
          "strength": "required",
          "valueSet": "http://octofhir.io/ValueSet/grant-types"
        }
      },
      {
        "id": "Client.scopes",
        "path": "Client.scopes",
        "min": 0,
        "max": "*",
        "type": [{ "code": "string" }],
        "comment": "Allowed SMART scopes"
      },
      {
        "id": "Client.jwksUri",
        "path": "Client.jwksUri",
        "min": 0,
        "max": "1",
        "type": [{ "code": "uri" }],
        "comment": "For confidential-asymmetric clients"
      },
      {
        "id": "Client.jwks",
        "path": "Client.jwks",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }],
        "comment": "Inline JWKS JSON for confidential-asymmetric clients"
      },
      {
        "id": "Client.active",
        "path": "Client.active",
        "min": 1,
        "max": "1",
        "type": [{ "code": "boolean" }]
      },
      {
        "id": "Client.trusted",
        "path": "Client.trusted",
        "min": 0,
        "max": "1",
        "type": [{ "code": "boolean" }],
        "comment": "Trusted clients may bypass certain policy checks"
      },
      {
        "id": "Client.accessPolicy",
        "path": "Client.accessPolicy",
        "min": 0,
        "max": "*",
        "type": [{ "code": "Reference", "targetProfile": ["http://octofhir.io/StructureDefinition/AccessPolicy"] }]
      }
    ]
  }
}
```

#### 4.1.2 AccessPolicy Resource

```json
{
  "resourceType": "StructureDefinition",
  "id": "AccessPolicy",
  "url": "http://octofhir.io/StructureDefinition/AccessPolicy",
  "name": "AccessPolicy",
  "title": "Access Policy Definition",
  "status": "active",
  "kind": "resource",
  "abstract": false,
  "type": "AccessPolicy",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/DomainResource",
  "derivation": "specialization",
  "differential": {
    "element": [
      {
        "id": "AccessPolicy.name",
        "path": "AccessPolicy.name",
        "min": 1,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "AccessPolicy.description",
        "path": "AccessPolicy.description",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "AccessPolicy.active",
        "path": "AccessPolicy.active",
        "min": 1,
        "max": "1",
        "type": [{ "code": "boolean" }]
      },
      {
        "id": "AccessPolicy.priority",
        "path": "AccessPolicy.priority",
        "min": 0,
        "max": "1",
        "type": [{ "code": "integer" }],
        "comment": "Lower numbers evaluated first; default 100"
      },
      {
        "id": "AccessPolicy.matcher",
        "path": "AccessPolicy.matcher",
        "min": 0,
        "max": "1",
        "type": [{ "code": "BackboneElement" }]
      },
      {
        "id": "AccessPolicy.matcher.clients",
        "path": "AccessPolicy.matcher.clients",
        "min": 0,
        "max": "*",
        "type": [{ "code": "string" }],
        "comment": "Client ID patterns (supports wildcards)"
      },
      {
        "id": "AccessPolicy.matcher.roles",
        "path": "AccessPolicy.matcher.roles",
        "min": 0,
        "max": "*",
        "type": [{ "code": "string" }]
      },
      {
        "id": "AccessPolicy.matcher.userTypes",
        "path": "AccessPolicy.matcher.userTypes",
        "min": 0,
        "max": "*",
        "type": [{ "code": "code" }],
        "comment": "Practitioner | Patient | RelatedPerson | Person"
      },
      {
        "id": "AccessPolicy.matcher.resourceTypes",
        "path": "AccessPolicy.matcher.resourceTypes",
        "min": 0,
        "max": "*",
        "type": [{ "code": "code" }]
      },
      {
        "id": "AccessPolicy.matcher.operations",
        "path": "AccessPolicy.matcher.operations",
        "min": 0,
        "max": "*",
        "type": [{ "code": "code" }],
        "binding": {
          "strength": "required",
          "valueSet": "http://hl7.org/fhir/ValueSet/type-restful-interaction"
        }
      },
      {
        "id": "AccessPolicy.engine",
        "path": "AccessPolicy.engine",
        "min": 1,
        "max": "1",
        "type": [{ "code": "BackboneElement" }]
      },
      {
        "id": "AccessPolicy.engine.type",
        "path": "AccessPolicy.engine.type",
        "min": 1,
        "max": "1",
        "type": [{ "code": "code" }],
        "binding": {
          "strength": "required",
          "valueSet": "http://octofhir.io/ValueSet/policy-engine-types"
        },
        "comment": "allow | deny | rhai | quickjs"
      },
      {
        "id": "AccessPolicy.engine.script",
        "path": "AccessPolicy.engine.script",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }],
        "comment": "Script content for rhai/quickjs engines"
      },
      {
        "id": "AccessPolicy.denyMessage",
        "path": "AccessPolicy.denyMessage",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }],
        "comment": "Message returned when policy denies access"
      }
    ]
  }
}
```

#### 4.1.3 IdentityProvider Resource

```json
{
  "resourceType": "StructureDefinition",
  "id": "IdentityProvider",
  "url": "http://octofhir.io/StructureDefinition/IdentityProvider",
  "name": "IdentityProvider",
  "title": "External Identity Provider Configuration",
  "status": "active",
  "kind": "resource",
  "abstract": false,
  "type": "IdentityProvider",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/DomainResource",
  "derivation": "specialization",
  "differential": {
    "element": [
      {
        "id": "IdentityProvider.name",
        "path": "IdentityProvider.name",
        "min": 1,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "IdentityProvider.type",
        "path": "IdentityProvider.type",
        "min": 1,
        "max": "1",
        "type": [{ "code": "code" }],
        "comment": "oidc | saml"
      },
      {
        "id": "IdentityProvider.issuer",
        "path": "IdentityProvider.issuer",
        "min": 1,
        "max": "1",
        "type": [{ "code": "uri" }]
      },
      {
        "id": "IdentityProvider.clientId",
        "path": "IdentityProvider.clientId",
        "min": 1,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "IdentityProvider.clientSecret",
        "path": "IdentityProvider.clientSecret",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "IdentityProvider.discoveryUrl",
        "path": "IdentityProvider.discoveryUrl",
        "min": 0,
        "max": "1",
        "type": [{ "code": "uri" }],
        "comment": "OIDC discovery endpoint"
      },
      {
        "id": "IdentityProvider.userMapping",
        "path": "IdentityProvider.userMapping",
        "min": 0,
        "max": "1",
        "type": [{ "code": "BackboneElement" }]
      },
      {
        "id": "IdentityProvider.userMapping.subjectClaim",
        "path": "IdentityProvider.userMapping.subjectClaim",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }],
        "defaultValueString": "sub"
      },
      {
        "id": "IdentityProvider.userMapping.fhirResourceType",
        "path": "IdentityProvider.userMapping.fhirResourceType",
        "min": 0,
        "max": "1",
        "type": [{ "code": "code" }],
        "comment": "Practitioner | Patient | RelatedPerson | Person"
      },
      {
        "id": "IdentityProvider.userMapping.autoProvision",
        "path": "IdentityProvider.userMapping.autoProvision",
        "min": 0,
        "max": "1",
        "type": [{ "code": "boolean" }],
        "defaultValueBoolean": false
      },
      {
        "id": "IdentityProvider.active",
        "path": "IdentityProvider.active",
        "min": 1,
        "max": "1",
        "type": [{ "code": "boolean" }]
      }
    ]
  }
}
```

#### 4.1.4 User Resource

```json
{
  "resourceType": "StructureDefinition",
  "id": "User",
  "url": "http://octofhir.io/StructureDefinition/User",
  "name": "User",
  "title": "Authentication User",
  "status": "active",
  "kind": "resource",
  "abstract": false,
  "type": "User",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/DomainResource",
  "derivation": "specialization",
  "differential": {
    "element": [
      {
        "id": "User.identifier",
        "path": "User.identifier",
        "min": 1,
        "max": "*",
        "type": [{ "code": "Identifier" }],
        "comment": "External IdP subject identifier(s)"
      },
      {
        "id": "User.email",
        "path": "User.email",
        "min": 0,
        "max": "1",
        "type": [{ "code": "string" }]
      },
      {
        "id": "User.name",
        "path": "User.name",
        "min": 0,
        "max": "1",
        "type": [{ "code": "HumanName" }]
      },
      {
        "id": "User.fhirUser",
        "path": "User.fhirUser",
        "min": 0,
        "max": "1",
        "type": [{ "code": "Reference", "targetProfile": [
          "http://hl7.org/fhir/StructureDefinition/Practitioner",
          "http://hl7.org/fhir/StructureDefinition/Patient",
          "http://hl7.org/fhir/StructureDefinition/RelatedPerson",
          "http://hl7.org/fhir/StructureDefinition/Person"
        ]}]
      },
      {
        "id": "User.roles",
        "path": "User.roles",
        "min": 0,
        "max": "*",
        "type": [{ "code": "string" }]
      },
      {
        "id": "User.identityProvider",
        "path": "User.identityProvider",
        "min": 0,
        "max": "1",
        "type": [{ "code": "Reference", "targetProfile": ["http://octofhir.io/StructureDefinition/IdentityProvider"] }]
      },
      {
        "id": "User.active",
        "path": "User.active",
        "min": 1,
        "max": "1",
        "type": [{ "code": "boolean" }]
      },
      {
        "id": "User.accessPolicy",
        "path": "User.accessPolicy",
        "min": 0,
        "max": "*",
        "type": [{ "code": "Reference", "targetProfile": ["http://octofhir.io/StructureDefinition/AccessPolicy"] }]
      }
    ]
  }
}
```

---

## 5. Data Models

### 5.1 Database Schema (PostgreSQL)

```sql
-- Auth schema extension for octofhir
CREATE SCHEMA IF NOT EXISTS octofhir_auth;

-- OAuth Clients
CREATE TABLE octofhir_auth.client (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id TEXT UNIQUE NOT NULL,
    txid BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    resource JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_client_client_id ON octofhir_auth.client ((resource->>'clientId'));
CREATE INDEX idx_client_active ON octofhir_auth.client ((resource->>'active'));

-- Users (linked to external IdP)
CREATE TABLE octofhir_auth.user (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id TEXT UNIQUE NOT NULL,
    txid BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    resource JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_user_email ON octofhir_auth.user ((resource->>'email'));
CREATE INDEX idx_user_identifier ON octofhir_auth.user USING GIN ((resource->'identifier'));

-- Identity Providers
CREATE TABLE octofhir_auth.identity_provider (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id TEXT UNIQUE NOT NULL,
    txid BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    resource JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Access Policies
CREATE TABLE octofhir_auth.access_policy (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id TEXT UNIQUE NOT NULL,
    txid BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    resource JSONB NOT NULL,
    priority INTEGER DEFAULT 100,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_access_policy_priority ON octofhir_auth.access_policy (priority);
CREATE INDEX idx_access_policy_active ON octofhir_auth.access_policy ((resource->>'active'));

-- Authorization Sessions (short-lived, for auth code flow)
CREATE TABLE octofhir_auth.authorization_session (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT UNIQUE NOT NULL,
    client_id TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL,
    state TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    code_challenge_method TEXT NOT NULL DEFAULT 'S256',
    user_id UUID REFERENCES octofhir_auth.user(id),
    launch_context JSONB,
    nonce TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ
);

CREATE INDEX idx_auth_session_code ON octofhir_auth.authorization_session (code) WHERE consumed_at IS NULL;
CREATE INDEX idx_auth_session_expires ON octofhir_auth.authorization_session (expires_at);

-- Refresh Tokens
CREATE TABLE octofhir_auth.refresh_token (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash TEXT UNIQUE NOT NULL,
    client_id TEXT NOT NULL,
    user_id UUID REFERENCES octofhir_auth.user(id),
    scope TEXT NOT NULL,
    launch_context JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);

CREATE INDEX idx_refresh_token_hash ON octofhir_auth.refresh_token (token_hash) WHERE revoked_at IS NULL;

-- Token Revocation (for JTI tracking)
CREATE TABLE octofhir_auth.revoked_token (
    jti TEXT PRIMARY KEY,
    revoked_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_revoked_token_expires ON octofhir_auth.revoked_token (expires_at);

-- JWKS Key Pairs (for signing)
CREATE TABLE octofhir_auth.signing_key (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kid TEXT UNIQUE NOT NULL,
    algorithm TEXT NOT NULL,
    public_key TEXT NOT NULL,
    private_key TEXT NOT NULL,  -- Encrypted at rest
    active BOOLEAN DEFAULT true,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    rotated_at TIMESTAMPTZ
);

-- Cleanup job for expired data
CREATE OR REPLACE FUNCTION octofhir_auth.cleanup_expired()
RETURNS void AS $$
BEGIN
    DELETE FROM octofhir_auth.authorization_session WHERE expires_at < NOW();
    DELETE FROM octofhir_auth.refresh_token WHERE expires_at < NOW() OR revoked_at < NOW() - INTERVAL '30 days';
    DELETE FROM octofhir_auth.revoked_token WHERE expires_at < NOW();
END;
$$ LANGUAGE plpgsql;

-- Trigger for hot-reload notifications
CREATE OR REPLACE FUNCTION octofhir_auth.notify_policy_change()
RETURNS trigger AS $$
BEGIN
    NOTIFY octofhir_auth_policy_change;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER access_policy_change_trigger
    AFTER INSERT OR UPDATE OR DELETE ON octofhir_auth.access_policy
    FOR EACH STATEMENT EXECUTE FUNCTION octofhir_auth.notify_policy_change();
```

---

## 6. API Endpoints

### 6.1 OAuth 2.0 Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/auth/authorize` | GET/POST | Authorization endpoint |
| `/auth/token` | POST | Token endpoint |
| `/auth/revoke` | POST | Token revocation (RFC 7009) |
| `/auth/introspect` | POST | Token introspection (RFC 7662) |
| `/auth/userinfo` | GET | OpenID Connect userinfo |
| `/auth/register` | POST | Dynamic client registration |
| `/auth/logout` | GET/POST | Session logout |

### 6.2 SMART Discovery Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/.well-known/smart-configuration` | GET | SMART configuration |
| `/.well-known/jwks.json` | GET | Server's public keys |
| `/.well-known/openid-configuration` | GET | OIDC discovery (alias) |

### 6.3 Admin Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/auth/admin/clients` | CRUD | Client management |
| `/auth/admin/users` | CRUD | User management |
| `/auth/admin/policies` | CRUD | AccessPolicy management |
| `/auth/admin/providers` | CRUD | IdentityProvider management |
| `/auth/admin/sessions` | GET/DELETE | Active sessions |

---

## 7. Security Considerations

### 7.1 Transport Security

- **TLS Required**: All auth endpoints require HTTPS in production
- **HSTS**: Strict-Transport-Security header enabled
- **Certificate Validation**: Full chain validation for external IdPs

### 7.2 Token Security

- **Short-Lived Access Tokens**: 5-60 minute expiration (configurable)
- **Secure Refresh Tokens**: Hashed in database, rotated on use
- **JTI Tracking**: All tokens have unique IDs for revocation
- **Algorithm Restrictions**: RS256/RS384/ES384 only; no symmetric

### 7.3 PKCE Enforcement

- **S256 Required**: Plain PKCE method explicitly forbidden
- **All Flows**: PKCE required for authorization code flow

### 7.4 Script Engine Security

- **Rhai**: Built-in sandboxing, operation limits, no eval()
- **QuickJS**: Memory limits, execution timeouts via interrupt handler
- **No Network/FS**: Neither engine has network or filesystem access
- **Alpine Compatible**: Both engines work on musl libc

### 7.5 Rate Limiting

- **Token Endpoint**: Rate limited per client/IP
- **Authorization Endpoint**: Rate limited per user/IP
- **Failed Attempts**: Exponential backoff on failures

---

## 8. Integration Points

### 8.1 Middleware Integration

```rust
// crates/octofhir-server/src/server.rs

use octofhir_auth::middleware::{AuthMiddleware, AuthConfig};

pub fn build_router(state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new(AuthConfig {
        required: true,
        allow_anonymous_read: false,
        ..Default::default()
    });

    Router::new()
        // Public endpoints (no auth)
        .route("/metadata", get(metadata_handler))
        .route("/.well-known/smart-configuration", get(smart_config_handler))
        .route("/.well-known/jwks.json", get(jwks_handler))

        // Auth endpoints
        .nest("/auth", auth_routes())

        // Protected FHIR endpoints
        .route("/:resource_type", get(search_handler).post(create_handler))
        .route("/:resource_type/:id", get(read_handler).put(update_handler).delete(delete_handler))
        .layer(auth_middleware)
        .with_state(state)
}
```

### 8.2 CapabilityStatement Extensions

```rust
// Add security extensions to CapabilityStatement
pub fn build_capability_statement(config: &Config) -> serde_json::Value {
    json!({
        "resourceType": "CapabilityStatement",
        // ... base fields ...
        "rest": [{
            "mode": "server",
            "security": {
                "cors": true,
                "service": [{
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/restful-security-service",
                        "code": "SMART-on-FHIR",
                        "display": "SMART-on-FHIR"
                    }]
                }],
                "extension": [{
                    "url": "http://fhir-registry.smarthealthit.org/StructureDefinition/oauth-uris",
                    "extension": [
                        {"url": "authorize", "valueUri": format!("{}/auth/authorize", config.base_url)},
                        {"url": "token", "valueUri": format!("{}/auth/token", config.base_url)},
                        {"url": "revoke", "valueUri": format!("{}/auth/revoke", config.base_url)},
                        {"url": "introspect", "valueUri": format!("{}/auth/introspect", config.base_url)}
                    ]
                }]
            }
        }]
    })
}
```

---

## 9. Configuration

### 9.1 Configuration Schema

```toml
# octofhir.toml - Auth section

[auth]
enabled = true
issuer = "https://fhir.example.com"

[auth.oauth]
authorization_code_lifetime_seconds = 600
access_token_lifetime_seconds = 3600
refresh_token_lifetime_days = 90
refresh_token_rotation = true
grant_types = ["authorization_code", "client_credentials", "refresh_token"]

[auth.smart]
launch_ehr_enabled = true
launch_standalone_enabled = true
public_clients_allowed = true
confidential_symmetric_allowed = true
confidential_asymmetric_allowed = true
refresh_tokens_enabled = true
openid_enabled = true
dynamic_registration_enabled = false
supported_scopes = [
    "openid", "fhirUser", "launch", "launch/patient", "launch/encounter",
    "offline_access", "online_access",
    "patient/*.cruds", "user/*.cruds", "system/*.cruds"
]

[auth.signing]
algorithm = "RS384"
key_rotation_days = 90
keys_to_keep = 3

[auth.policy]
default_deny = true
rhai_enabled = true
quickjs_enabled = true

[auth.policy.rhai]
max_operations = 10000
max_call_levels = 32

[auth.policy.quickjs]
memory_limit_mb = 16
max_stack_size_kb = 256
timeout_ms = 100
pool_size = 4  # Number of runtime instances

[auth.federation]
allow_external_idp = true
auto_provision_users = false
jwks_cache_ttl_seconds = 3600

[auth.rate_limiting]
token_requests_per_minute = 60
auth_requests_per_minute = 30
max_failed_attempts = 5
lockout_duration_seconds = 300

[auth.audit]
log_successful_auth = true
log_failed_auth = true
log_access_decisions = true
```

---

## 10. Implementation Roadmap

### Phase 1: Core OAuth 2.0 Server (Foundation)

| Task | Description | Dependencies |
|------|-------------|--------------|
| 1.1 | Create `octofhir-auth` crate structure | None |
| 1.2 | Implement JWT generation/validation (RS256/RS384/ES384) | 1.1 |
| 1.3 | Implement PKCE (S256) | 1.1 |
| 1.4 | Create `octofhir-auth-postgres` crate | 1.1 |
| 1.5 | Implement database schema migrations | 1.4 |
| 1.6 | Implement Client storage | 1.4, 1.5 |
| 1.7 | Implement authorization code flow | 1.2, 1.3, 1.6 |
| 1.8 | Implement token endpoint | 1.7 |
| 1.9 | Implement client credentials flow | 1.2, 1.6 |
| 1.10 | Implement refresh token flow | 1.8 |
| 1.11 | Implement token revocation (RFC 7009) | 1.8 |
| 1.12 | Implement token introspection (RFC 7662) | 1.8 |
| 1.13 | Add auth middleware to Axum | 1.8 |
| 1.14 | Add configuration support | 1.1 |

### Phase 2: SMART on FHIR Compliance

| Task | Description | Dependencies |
|------|-------------|--------------|
| 2.1 | Implement SMART scope parser (v2 syntax) | 1.1 |
| 2.2 | Implement scope validation | 2.1 |
| 2.3 | Implement launch context storage | 1.5 |
| 2.4 | Implement EHR launch flow | 1.7, 2.3 |
| 2.5 | Implement standalone launch flow | 1.7, 2.3 |
| 2.6 | Implement `.well-known/smart-configuration` | 2.1 |
| 2.7 | Implement `.well-known/jwks.json` | 1.2 |
| 2.8 | Add CapabilityStatement security extensions | 2.6 |
| 2.9 | Implement OpenID Connect (id_token, fhirUser) | 1.8 |
| 2.10 | Implement userinfo endpoint | 2.9 |

### Phase 3: External Identity Federation

| Task | Description | Dependencies |
|------|-------------|--------------|
| 3.1 | Implement OIDC discovery client | None |
| 3.2 | Implement JWKS fetching and caching | 3.1 |
| 3.3 | Implement external IdP authentication flow | 3.1, 3.2 |
| 3.4 | Implement user provisioning/linking | 3.3 |
| 3.5 | Create IdentityProvider resource type | 3.3 |
| 3.6 | Create User resource type | 3.4 |
| 3.7 | Add IdP admin endpoints | 3.5, 3.6 |

### Phase 4: AccessPolicy Engine

| Task | Description | Dependencies |
|------|-------------|--------------|
| 4.1 | Design policy evaluation context | 1.13 |
| 4.2 | Implement pattern matcher | 4.1 |
| 4.3 | Create AccessPolicy resource type | 4.1 |
| 4.4 | Implement policy storage | 4.3 |
| 4.5 | Implement policy evaluation engine | 4.2, 4.4 |
| 4.6 | Integrate Rhai scripting engine | 4.5 |
| 4.7 | Integrate QuickJS (rquickjs) runtime | 4.5 |
| 4.8 | Implement compartment-based access | 4.5 |
| 4.9 | Implement policy hot-reload | 4.4 |
| 4.10 | Integrate policy engine with middleware | 4.5, 1.13 |

### Phase 5: Custom FHIR Resources (IG)

| Task | Description | Dependencies |
|------|-------------|--------------|
| 5.1 | Create Client StructureDefinition | 1.6 |
| 5.2 | Create AccessPolicy StructureDefinition | 4.3 |
| 5.3 | Create IdentityProvider StructureDefinition | 3.5 |
| 5.4 | Create User StructureDefinition | 3.6 |
| 5.5 | Create supporting ValueSets/CodeSystems | 5.1-5.4 |
| 5.6 | Add resources to octofhir-internal IG | 5.1-5.5 |

### Phase 6: Audit & Production Hardening

| Task | Description | Dependencies |
|------|-------------|--------------|
| 6.1 | Implement AuditEvent generation | None |
| 6.2 | Add authentication event logging | 6.1, 1.7 |
| 6.3 | Add authorization decision logging | 6.1, 4.5 |
| 6.4 | Implement rate limiting | 1.8 |
| 6.5 | Implement key rotation | 1.2 |
| 6.6 | Security testing (Inferno) | All |
| 6.7 | Performance testing | All |

---

## Appendix A: Example AccessPolicy Resources

### A.1 Allow Practitioners to Read All Observations

```json
{
  "resourceType": "AccessPolicy",
  "id": "practitioners-read-observations",
  "name": "Practitioners can read Observations",
  "active": true,
  "priority": 50,
  "matcher": {
    "userTypes": ["Practitioner"],
    "resourceTypes": ["Observation"],
    "operations": ["read", "search-type"]
  },
  "engine": {
    "type": "allow"
  }
}
```

### A.2 Patients Access Own Data Only (Rhai Script)

```json
{
  "resourceType": "AccessPolicy",
  "id": "patient-own-data-rhai",
  "name": "Patients can only access their own data (Rhai)",
  "active": true,
  "priority": 100,
  "matcher": {
    "userTypes": ["Patient"]
  },
  "engine": {
    "type": "rhai",
    "script": "let patient_id = user.fhir_user.split(\"/\")[1]; patient_context == patient_id || resource.subject.reference == `Patient/${patient_id}`"
  },
  "denyMessage": "Patients can only access their own data"
}
```

### A.3 Complex Organization-Based Access (QuickJS)

```json
{
  "resourceType": "AccessPolicy",
  "id": "organization-access-quickjs",
  "name": "Organization-based hierarchical access",
  "active": true,
  "priority": 75,
  "engine": {
    "type": "quickjs",
    "script": "// Check if user's organization has access\nconst userOrg = user?.attributes?.organization;\nconst resourceOrg = resource?.managingOrganization?.reference;\n\nif (!userOrg || !resourceOrg) {\n  return abstain();\n}\n\n// Simple equality check (extend for hierarchy)\nif (resourceOrg === userOrg || resourceOrg.endsWith(userOrg)) {\n  return allow();\n}\n\nreturn deny('Organization access denied');"
  }
}
```

---

## Appendix B: Rust Dependencies

```toml
# crates/octofhir-auth/Cargo.toml

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"

# Web framework
axum = { version = "0.8", features = ["json", "macros"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }

# JWT & Crypto
jsonwebtoken = { version = "10", features = ["aws_lc_rs"] }
rand = "0.8"
sha2 = "0.10"
base64 = "0.22"

# HTTP client (for IdP federation)
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Time
time = { version = "0.3", features = ["serde", "formatting"] }

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "time", "json"] }

# Scripting engines
rhai = { version = "1.19", features = ["sync"] }
rquickjs = { version = "0.6", features = ["bindgen", "classes", "properties"] }

# Synchronization
parking_lot = "0.12"

# Error handling
thiserror = "2"
anyhow = "1"

# Logging
tracing = "0.1"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
url = { version = "2", features = ["serde"] }
ipnetwork = "0.20"
regex = "1"
num_cpus = "1"

[dev-dependencies]
tokio-test = "0.4"
wiremock = "0.6"
```

---

## Appendix C: Script Engine Comparison

| Feature | Rhai | QuickJS (rquickjs) |
|---------|------|-------------------|
| **Language** | Rust-like DSL | JavaScript ES2020 |
| **Best For** | Simple conditions | Complex logic |
| **Memory Limit** | Via max_* settings | Built-in `set_memory_limit` |
| **Timeout** | Via max_operations | Interrupt handler |
| **Thread Safety** | Engine is `Send+Sync` | Mutex per runtime |
| **Alpine/musl** | Yes (pure Rust) | Yes (tested) |
| **Binary Size** | ~500KB | ~1MB |
| **Startup Time** | <1ms | ~50ms |

**Recommendation:**
- Use **Rhai** for simple role/attribute checks
- Use **QuickJS** for complex business logic or when JS familiarity is needed

---

## Appendix D: References

- [FHIR Security](https://build.fhir.org/security.html)
- [SMART App Launch IG v2.2.0](https://build.fhir.org/ig/HL7/smart-app-launch/)
- [OAuth 2.0 (RFC 6749)](https://tools.ietf.org/html/rfc6749)
- [PKCE (RFC 7636)](https://tools.ietf.org/html/rfc7636)
- [Token Introspection (RFC 7662)](https://tools.ietf.org/html/rfc7662)
- [Token Revocation (RFC 7009)](https://tools.ietf.org/html/rfc7009)
- [rquickjs GitHub](https://github.com/DelSkayn/rquickjs)
- [Rhai Scripting Language](https://rhai.rs/)
- [Aidbox Access Control](https://docs.aidbox.app/access-control/authorization)
- [Medplum Access Policies](https://www.medplum.com/docs/access/access-policies)
