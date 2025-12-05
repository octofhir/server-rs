//! SMART on FHIR v2 scope parsing and validation.
//!
//! This module implements parsing for SMART on FHIR v2 scope syntax as defined in
//! the [SMART App Launch Implementation Guide](https://build.fhir.org/ig/HL7/smart-app-launch/scopes-and-launch-context.html).
//!
//! # Scope Format
//!
//! SMART v2 scopes follow the format: `context/ResourceType.permissions?param=value`
//!
//! - **Context**: `patient`, `user`, or `system`
//! - **ResourceType**: A FHIR resource type (e.g., `Patient`, `Observation`) or `*` for wildcard
//! - **Permissions**: Ordered subset of `cruds` (create, read, update, delete, search)
//! - **Filter** (optional): Search parameter filter (e.g., `?category=laboratory`)
//!
//! # Examples
//!
//! ```
//! use octofhir_auth::smart::scopes::{SmartScopes, ScopeContext, ResourceType};
//!
//! let scopes = SmartScopes::parse("launch openid patient/Observation.rs").unwrap();
//! assert!(scopes.launch);
//! assert!(scopes.openid);
//! assert_eq!(scopes.resource_scopes.len(), 1);
//!
//! let scope = &scopes.resource_scopes[0];
//! assert_eq!(scope.context, ScopeContext::Patient);
//! assert!(scope.permissions.read);
//! assert!(scope.permissions.search);
//! ```

use std::fmt;
use std::str::FromStr;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during scope parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScopeError {
    /// The scope string format is invalid.
    #[error("Invalid scope format: {0}")]
    InvalidFormat(String),

    /// The context (patient/user/system) is invalid.
    #[error("Invalid context: {0}")]
    InvalidContext(String),

    /// An invalid permission character was encountered.
    #[error("Invalid permission character: {0}")]
    InvalidPermission(char),

    /// Permissions are not in the required order (c < r < u < d < s).
    #[error("Permissions must be in order: c < r < u < d < s")]
    InvalidPermissionOrder,

    /// The scope string is empty.
    #[error("Empty scope string")]
    Empty,

    /// Requested scope exceeds allowed scopes.
    #[error("Scope {0} not permitted by allowed scopes")]
    ScopeNotPermitted(String),
}

// ============================================================================
// Standalone Launch Context Requirements
// ============================================================================

/// Requirements for context selection during standalone launch.
///
/// When an app launches independently (not from within an EHR), it may request
/// patient or encounter context by including `launch/patient` or `launch/encounter`
/// scopes. The authorization server should prompt the user to select the
/// appropriate context.
///
/// # Usage
///
/// ```
/// use octofhir_auth::smart::scopes::SmartScopes;
///
/// let scopes = SmartScopes::parse("launch/patient patient/Patient.rs").unwrap();
/// let requirements = scopes.context_requirements();
///
/// if requirements.needs_patient_selection {
///     // Show patient picker in authorization UI
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StandaloneContextRequirements {
    /// Whether the user needs to select a patient during authorization.
    /// True when `launch/patient` scope is requested.
    pub needs_patient_selection: bool,

    /// Whether the user needs to select an encounter during authorization.
    /// True when `launch/encounter` scope is requested.
    pub needs_encounter_selection: bool,
}

impl StandaloneContextRequirements {
    /// Returns true if any context selection is needed.
    #[must_use]
    pub fn needs_any_selection(&self) -> bool {
        self.needs_patient_selection || self.needs_encounter_selection
    }
}

// ============================================================================
// FHIR Operation
// ============================================================================

/// FHIR RESTful operations that can be performed on resources.
///
/// Each operation maps to a required permission (c, r, u, d, or s).
/// Some operations like `Capabilities` are always allowed without checking scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FhirOperation {
    /// Read a resource by ID (GET /[type]/[id])
    Read,
    /// Read a specific version (GET /[type]/[id]/_history/[vid])
    VRead,
    /// Update a resource (PUT /[type]/[id])
    Update,
    /// Patch a resource (PATCH /[type]/[id])
    Patch,
    /// Delete a resource (DELETE /[type]/[id])
    Delete,
    /// Create a resource (POST /[type])
    Create,
    /// Search with GET (GET /[type]?params)
    Search,
    /// Type-level search (POST /[type]/_search)
    SearchType,
    /// System-level search (GET /?params or POST /_search)
    SearchSystem,
    /// Capabilities/conformance statement (GET /metadata)
    Capabilities,
    /// Batch operation (POST / with Bundle type=batch)
    Batch,
    /// Transaction operation (POST / with Bundle type=transaction)
    Transaction,
    /// History of a resource instance (GET /[type]/[id]/_history)
    HistoryInstance,
    /// History of a resource type (GET /[type]/_history)
    HistoryType,
    /// System history (GET /_history)
    HistorySystem,
    /// Named operation ($operation)
    Operation,
}

impl FhirOperation {
    /// Returns the permission character required for this operation.
    ///
    /// Returns `None` for operations that don't require scope checking
    /// (e.g., `Capabilities` is always allowed).
    #[must_use]
    pub fn required_permission(&self) -> Option<char> {
        match self {
            // Read operations require 'r'
            Self::Read | Self::VRead => Some('r'),

            // Write operations
            Self::Create => Some('c'),
            Self::Update | Self::Patch => Some('u'),
            Self::Delete => Some('d'),

            // Search operations require 's'
            Self::Search | Self::SearchType | Self::SearchSystem => Some('s'),

            // History operations require 'r' (read access to see history)
            Self::HistoryInstance | Self::HistoryType | Self::HistorySystem => Some('r'),

            // Batch/Transaction - handled specially (check individual entries)
            // Return None here; actual permission checking happens per-entry
            Self::Batch | Self::Transaction => None,

            // Operations - permission depends on the specific operation
            // Return None here; actual permission depends on operation definition
            Self::Operation => None,

            // Capabilities is always allowed
            Self::Capabilities => None,
        }
    }

    /// Returns true if this operation is always allowed without scope checking.
    #[must_use]
    pub fn always_allowed(&self) -> bool {
        matches!(self, Self::Capabilities)
    }

    /// Returns true if this operation requires instance-level access.
    #[must_use]
    pub fn is_instance_level(&self) -> bool {
        matches!(
            self,
            Self::Read
                | Self::VRead
                | Self::Update
                | Self::Patch
                | Self::Delete
                | Self::HistoryInstance
        )
    }

    /// Returns true if this operation requires type-level access.
    #[must_use]
    pub fn is_type_level(&self) -> bool {
        matches!(
            self,
            Self::Create | Self::Search | Self::SearchType | Self::HistoryType
        )
    }

    /// Returns true if this operation requires system-level access.
    #[must_use]
    pub fn is_system_level(&self) -> bool {
        matches!(
            self,
            Self::SearchSystem
                | Self::HistorySystem
                | Self::Batch
                | Self::Transaction
                | Self::Capabilities
        )
    }
}

impl fmt::Display for FhirOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::VRead => write!(f, "vread"),
            Self::Update => write!(f, "update"),
            Self::Patch => write!(f, "patch"),
            Self::Delete => write!(f, "delete"),
            Self::Create => write!(f, "create"),
            Self::Search => write!(f, "search"),
            Self::SearchType => write!(f, "search-type"),
            Self::SearchSystem => write!(f, "search-system"),
            Self::Capabilities => write!(f, "capabilities"),
            Self::Batch => write!(f, "batch"),
            Self::Transaction => write!(f, "transaction"),
            Self::HistoryInstance => write!(f, "history-instance"),
            Self::HistoryType => write!(f, "history-type"),
            Self::HistorySystem => write!(f, "history-system"),
            Self::Operation => write!(f, "operation"),
        }
    }
}

// ============================================================================
// Scope Context
// ============================================================================

/// The context in which a scope applies.
///
/// SMART on FHIR defines three contexts for resource access:
/// - `patient/*` - Access limited to a specific patient's data
/// - `user/*` - Access based on the authenticated user's permissions
/// - `system/*` - Backend service access (no user context)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeContext {
    /// Patient-level access (patient/*).
    /// Data access is restricted to resources associated with the launch patient.
    Patient,

    /// User-level access (user/*).
    /// Data access is based on the authenticated user's permissions.
    User,

    /// System-level access (system/*).
    /// For backend services operating without a user context.
    System,
}

impl ScopeContext {
    /// Returns the string representation of the context.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Patient => "patient",
            Self::User => "user",
            Self::System => "system",
        }
    }
}

impl fmt::Display for ScopeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Resource Type
// ============================================================================

/// The FHIR resource type targeted by a scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// A specific FHIR resource type (e.g., "Patient", "Observation").
    Specific(String),

    /// Wildcard (*) matching all resource types.
    Wildcard,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Specific(s) => write!(f, "{}", s),
            Self::Wildcard => write!(f, "*"),
        }
    }
}

// ============================================================================
// Permissions
// ============================================================================

/// FHIR operation permissions (CRUDS).
///
/// Permissions control which operations are allowed on resources.
/// The canonical order is: create (c) < read (r) < update (u) < delete (d) < search (s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Permissions {
    /// Create permission ('c').
    pub create: bool,
    /// Read permission ('r').
    pub read: bool,
    /// Update permission ('u').
    pub update: bool,
    /// Delete permission ('d').
    pub delete: bool,
    /// Search permission ('s').
    pub search: bool,
}

impl Permissions {
    /// Returns true if read permission is granted.
    #[must_use]
    pub fn can_read(&self) -> bool {
        self.read
    }

    /// Returns true if any write permission is granted (create, update, or delete).
    #[must_use]
    pub fn can_write(&self) -> bool {
        self.create || self.update || self.delete
    }

    /// Returns true if search permission is granted.
    #[must_use]
    pub fn can_search(&self) -> bool {
        self.search
    }

    /// Returns true if all permissions are granted.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.create && self.read && self.update && self.delete && self.search
    }
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.create {
            write!(f, "c")?;
        }
        if self.read {
            write!(f, "r")?;
        }
        if self.update {
            write!(f, "u")?;
        }
        if self.delete {
            write!(f, "d")?;
        }
        if self.search {
            write!(f, "s")?;
        }
        Ok(())
    }
}

impl FromStr for Permissions {
    type Err = ScopeError;

    /// Parse permissions from a CRUDS string.
    ///
    /// Characters must be in order: c < r < u < d < s.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::str::FromStr;
    /// use octofhir_auth::smart::scopes::Permissions;
    ///
    /// let perms = Permissions::from_str("crus").unwrap();
    /// assert!(perms.create);
    /// assert!(perms.read);
    /// assert!(perms.update);
    /// assert!(!perms.delete);
    /// assert!(perms.search);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns `ScopeError::InvalidPermission` for unknown characters.
    /// Returns `ScopeError::InvalidPermissionOrder` if characters are out of order.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut perms = Self::default();
        let mut last_order: Option<u8> = None;

        // Helper to get the logical order of permissions (c=1, r=2, u=3, d=4, s=5)
        fn perm_order(c: char) -> Option<u8> {
            match c {
                'c' => Some(1),
                'r' => Some(2),
                'u' => Some(3),
                'd' => Some(4),
                's' => Some(5),
                _ => None,
            }
        }

        for c in s.chars() {
            let order = perm_order(c).ok_or(ScopeError::InvalidPermission(c))?;

            // Validate ordering - each permission must be after the previous in CRUDS order
            if let Some(prev_order) = last_order
                && order <= prev_order
            {
                return Err(ScopeError::InvalidPermissionOrder);
            }

            match c {
                'c' => perms.create = true,
                'r' => perms.read = true,
                'u' => perms.update = true,
                'd' => perms.delete = true,
                's' => perms.search = true,
                _ => unreachable!(), // Already handled above
            }
            last_order = Some(order);
        }

        Ok(perms)
    }
}

// ============================================================================
// Scope Filter
// ============================================================================

/// A search parameter filter applied to a scope.
///
/// Filters allow scopes to be narrowed to specific data subsets.
/// For example, `patient/Observation.rs?category=laboratory` limits access
/// to laboratory observations only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFilter {
    /// The search parameter name (e.g., "category").
    pub parameter: String,
    /// The parameter value (e.g., "laboratory").
    pub value: String,
}

impl fmt::Display for ScopeFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.parameter, self.value)
    }
}

// ============================================================================
// Smart Scope
// ============================================================================

/// A parsed SMART v2 resource scope.
///
/// Represents a scope in the format: `context/ResourceType.permissions?filter`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartScope {
    /// The scope context (patient, user, or system).
    pub context: ScopeContext,
    /// The target resource type or wildcard.
    pub resource_type: ResourceType,
    /// The granted permissions (CRUDS).
    pub permissions: Permissions,
    /// Optional filter to narrow the scope.
    pub filter: Option<ScopeFilter>,
}

impl SmartScope {
    /// Parse a resource scope string.
    ///
    /// Format: `context/ResourceType.permissions?param=value`
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::{SmartScope, ScopeContext, ResourceType};
    ///
    /// let scope = SmartScope::parse("patient/Observation.rs?category=laboratory").unwrap();
    /// assert_eq!(scope.context, ScopeContext::Patient);
    /// assert!(matches!(scope.resource_type, ResourceType::Specific(ref s) if s == "Observation"));
    /// assert!(scope.permissions.read);
    /// assert!(scope.permissions.search);
    /// assert!(scope.filter.is_some());
    /// ```
    pub fn parse(s: &str) -> Result<Self, ScopeError> {
        // Split by '/' for context
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(ScopeError::InvalidFormat(s.to_string()));
        }

        let context = match parts[0] {
            "patient" => ScopeContext::Patient,
            "user" => ScopeContext::User,
            "system" => ScopeContext::System,
            _ => return Err(ScopeError::InvalidContext(parts[0].to_string())),
        };

        // Split by '?' for filter
        let (resource_perm, filter_str) = if let Some(idx) = parts[1].find('?') {
            let (rp, f) = parts[1].split_at(idx);
            (rp, Some(&f[1..])) // Skip '?'
        } else {
            (parts[1], None)
        };

        // Split by '.' for resource type and permissions
        let rp_parts: Vec<&str> = resource_perm.splitn(2, '.').collect();
        if rp_parts.len() != 2 {
            return Err(ScopeError::InvalidFormat(s.to_string()));
        }

        let resource_type = if rp_parts[0] == "*" {
            ResourceType::Wildcard
        } else {
            ResourceType::Specific(rp_parts[0].to_string())
        };

        let permissions = Permissions::from_str(rp_parts[1])?;

        let filter = filter_str.map(|f| {
            let filter_parts: Vec<&str> = f.splitn(2, '=').collect();
            ScopeFilter {
                parameter: filter_parts.first().unwrap_or(&"").to_string(),
                value: filter_parts.get(1).unwrap_or(&"").to_string(),
            }
        });

        Ok(Self {
            context,
            resource_type,
            permissions,
            filter,
        })
    }

    /// Check if this scope grants access to a specific resource type.
    #[must_use]
    pub fn matches_resource(&self, resource_type: &str) -> bool {
        match &self.resource_type {
            ResourceType::Wildcard => true,
            ResourceType::Specific(rt) => rt == resource_type,
        }
    }

    /// Check if this scope permits a specific FHIR operation on a resource.
    ///
    /// # Arguments
    ///
    /// * `resource_type` - The FHIR resource type being accessed
    /// * `operation` - The FHIR operation being performed
    /// * `patient_context` - The current patient context (required for patient/* scopes)
    ///
    /// # Returns
    ///
    /// Returns `true` if this scope permits the operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::{SmartScope, FhirOperation};
    ///
    /// let scope = SmartScope::parse("patient/Observation.rs").unwrap();
    ///
    /// // Permits read with patient context
    /// assert!(scope.matches("Observation", FhirOperation::Read, Some("patient-123")));
    ///
    /// // Does not permit without patient context (patient/* scopes require it)
    /// assert!(!scope.matches("Observation", FhirOperation::Read, None));
    ///
    /// // Does not permit create (no 'c' permission)
    /// assert!(!scope.matches("Observation", FhirOperation::Create, Some("patient-123")));
    /// ```
    #[must_use]
    pub fn matches(
        &self,
        resource_type: &str,
        operation: FhirOperation,
        patient_context: Option<&str>,
    ) -> bool {
        // Always-allowed operations don't need scope checking
        if operation.always_allowed() {
            return true;
        }

        // Check resource type matches
        if !self.matches_resource(resource_type) {
            return false;
        }

        // Check patient context requirement for patient/* scopes
        if self.context == ScopeContext::Patient && patient_context.is_none() {
            return false;
        }

        // Check permission based on operation
        if let Some(required_perm) = operation.required_permission() {
            self.has_permission(required_perm)
        } else {
            // Operations without required permission (Capabilities, Batch, Transaction, Operation)
            // are handled specially - return true here and let caller handle specifics
            true
        }
    }

    /// Check if this scope has a specific permission character.
    #[must_use]
    pub fn has_permission(&self, perm: char) -> bool {
        match perm {
            'c' => self.permissions.create,
            'r' => self.permissions.read,
            'u' => self.permissions.update,
            'd' => self.permissions.delete,
            's' => self.permissions.search,
            _ => false,
        }
    }

    /// Check if this scope covers another scope (for validation).
    ///
    /// A scope covers another if it grants at least the same permissions
    /// for the same or broader resource access.
    #[must_use]
    pub fn covers(&self, other: &SmartScope) -> bool {
        // Context must match
        if self.context != other.context {
            return false;
        }

        // Check resource type coverage
        let resource_covered = match (&self.resource_type, &other.resource_type) {
            (ResourceType::Wildcard, _) => true,
            (ResourceType::Specific(a), ResourceType::Specific(b)) => a == b,
            (ResourceType::Specific(_), ResourceType::Wildcard) => false,
        };

        if !resource_covered {
            return false;
        }

        // Check all permissions in other are covered by self
        if other.permissions.create && !self.permissions.create {
            return false;
        }
        if other.permissions.read && !self.permissions.read {
            return false;
        }
        if other.permissions.update && !self.permissions.update {
            return false;
        }
        if other.permissions.delete && !self.permissions.delete {
            return false;
        }
        if other.permissions.search && !self.permissions.search {
            return false;
        }

        // Filter handling: if self has a filter, other must have compatible filter
        // For now, exact filter match or self has no filter
        match (&self.filter, &other.filter) {
            (None, _) => true,
            (Some(a), Some(b)) => a == b,
            (Some(_), None) => false,
        }
    }

    /// Compute the intersection of two scopes.
    ///
    /// Returns `Some(scope)` if the scopes overlap (same context and resource type),
    /// with permissions being the intersection of both.
    /// Returns `None` if the scopes don't overlap.
    #[must_use]
    pub fn intersect(&self, other: &SmartScope) -> Option<SmartScope> {
        // Context must match
        if self.context != other.context {
            return None;
        }

        // Find resource type intersection
        let resource_type = match (&self.resource_type, &other.resource_type) {
            (ResourceType::Wildcard, rt) => rt.clone(),
            (rt, ResourceType::Wildcard) => rt.clone(),
            (ResourceType::Specific(a), ResourceType::Specific(b)) if a == b => {
                ResourceType::Specific(a.clone())
            }
            _ => return None,
        };

        // Intersect permissions
        let permissions = Permissions {
            create: self.permissions.create && other.permissions.create,
            read: self.permissions.read && other.permissions.read,
            update: self.permissions.update && other.permissions.update,
            delete: self.permissions.delete && other.permissions.delete,
            search: self.permissions.search && other.permissions.search,
        };

        // If no permissions remain, no intersection
        if !permissions.create
            && !permissions.read
            && !permissions.update
            && !permissions.delete
            && !permissions.search
        {
            return None;
        }

        // Intersect filters (must be identical or one is None)
        let filter = match (&self.filter, &other.filter) {
            (None, None) => None,
            (Some(f), None) | (None, Some(f)) => Some(f.clone()),
            (Some(a), Some(b)) if a == b => Some(a.clone()),
            _ => return None, // Incompatible filters
        };

        Some(SmartScope {
            context: self.context,
            resource_type,
            permissions,
            filter,
        })
    }
}

impl fmt::Display for SmartScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}.{}",
            self.context, self.resource_type, self.permissions
        )?;
        if let Some(ref filter) = self.filter {
            write!(f, "?{}", filter)?;
        }
        Ok(())
    }
}

// ============================================================================
// Smart Scopes Collection
// ============================================================================

/// A collection of parsed SMART scopes.
///
/// This struct represents the full set of scopes granted to a client,
/// including resource scopes, launch scopes, and OpenID Connect scopes.
#[derive(Debug, Clone, Default)]
pub struct SmartScopes {
    /// Resource access scopes (e.g., patient/Observation.rs).
    pub resource_scopes: Vec<SmartScope>,

    /// EHR launch scope - app was launched from EHR.
    pub launch: bool,

    /// Request patient context on launch.
    pub launch_patient: bool,

    /// Request encounter context on launch.
    pub launch_encounter: bool,

    /// OpenID Connect scope - request id_token.
    pub openid: bool,

    /// Request fhirUser claim in id_token.
    pub fhir_user: bool,

    /// Request refresh token for offline access.
    pub offline_access: bool,

    /// Request refresh token valid only during online session.
    pub online_access: bool,
}

impl SmartScopes {
    /// Parse a space-separated scope string.
    ///
    /// Unknown scopes are silently ignored per the SMART specification.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::SmartScopes;
    ///
    /// let scopes = SmartScopes::parse("launch openid fhirUser patient/Patient.r offline_access").unwrap();
    /// assert!(scopes.launch);
    /// assert!(scopes.openid);
    /// assert!(scopes.fhir_user);
    /// assert!(scopes.offline_access);
    /// assert_eq!(scopes.resource_scopes.len(), 1);
    /// ```
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
                    // Try to parse as resource scope
                    if let Ok(resource_scope) = SmartScope::parse(s) {
                        scopes.resource_scopes.push(resource_scope);
                    }
                    // Unknown scopes are silently ignored per SMART spec
                }
            }
        }

        Ok(scopes)
    }

    /// Check if any resource scope grants access to a specific resource type.
    #[must_use]
    pub fn has_resource_access(&self, resource_type: &str) -> bool {
        self.resource_scopes
            .iter()
            .any(|s| s.matches_resource(resource_type))
    }

    /// Check if any resource scope grants read access to a specific resource type.
    #[must_use]
    pub fn can_read_resource(&self, resource_type: &str) -> bool {
        self.resource_scopes
            .iter()
            .any(|s| s.matches_resource(resource_type) && s.permissions.can_read())
    }

    /// Check if any resource scope grants write access to a specific resource type.
    #[must_use]
    pub fn can_write_resource(&self, resource_type: &str) -> bool {
        self.resource_scopes
            .iter()
            .any(|s| s.matches_resource(resource_type) && s.permissions.can_write())
    }

    /// Get all scopes for a specific context.
    pub fn scopes_for_context(&self, context: ScopeContext) -> impl Iterator<Item = &SmartScope> {
        self.resource_scopes
            .iter()
            .filter(move |s| s.context == context)
    }

    /// Returns true if refresh tokens should be issued.
    #[must_use]
    pub fn wants_refresh_token(&self) -> bool {
        self.offline_access || self.online_access
    }

    /// Returns true if the scope set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resource_scopes.is_empty()
            && !self.launch
            && !self.launch_patient
            && !self.launch_encounter
            && !self.openid
            && !self.fhir_user
            && !self.offline_access
            && !self.online_access
    }

    /// Returns the context selection requirements for standalone launch.
    ///
    /// For standalone launches (without an EHR launch parameter), the app may
    /// request patient or encounter context by including `launch/patient` or
    /// `launch/encounter` scopes. This method returns what context selection
    /// is required during the authorization flow.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::SmartScopes;
    ///
    /// // App requesting patient context
    /// let scopes = SmartScopes::parse("launch/patient patient/Patient.rs openid").unwrap();
    /// let reqs = scopes.context_requirements();
    /// assert!(reqs.needs_patient_selection);
    /// assert!(!reqs.needs_encounter_selection);
    ///
    /// // App not requesting any context
    /// let scopes = SmartScopes::parse("patient/Patient.rs openid").unwrap();
    /// let reqs = scopes.context_requirements();
    /// assert!(!reqs.needs_patient_selection);
    /// ```
    #[must_use]
    pub fn context_requirements(&self) -> StandaloneContextRequirements {
        StandaloneContextRequirements {
            needs_patient_selection: self.launch_patient,
            needs_encounter_selection: self.launch_encounter,
        }
    }

    /// Returns true if this is a standalone launch requiring context selection.
    ///
    /// A standalone launch requires context selection when `launch/patient` or
    /// `launch/encounter` is requested but not the `launch` scope (which indicates
    /// EHR launch with pre-selected context).
    #[must_use]
    pub fn is_standalone_with_context(&self) -> bool {
        !self.launch && (self.launch_patient || self.launch_encounter)
    }

    // -------------------------------------------------------------------------
    // Validation Methods
    // -------------------------------------------------------------------------

    /// Check if the scopes permit a specific FHIR operation on a resource.
    ///
    /// # Arguments
    ///
    /// * `resource_type` - The FHIR resource type being accessed
    /// * `operation` - The FHIR operation being performed
    /// * `patient_context` - The current patient context (required for patient/* scopes)
    ///
    /// # Returns
    ///
    /// Returns `true` if any scope permits the operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::{SmartScopes, FhirOperation};
    ///
    /// let scopes = SmartScopes::parse("patient/Observation.rs").unwrap();
    ///
    /// // Permits read with patient context
    /// assert!(scopes.permits("Observation", FhirOperation::Read, Some("patient-123")));
    ///
    /// // Does not permit create (no 'c' permission)
    /// assert!(!scopes.permits("Observation", FhirOperation::Create, Some("patient-123")));
    ///
    /// // Capabilities is always allowed
    /// assert!(scopes.permits("metadata", FhirOperation::Capabilities, None));
    /// ```
    #[must_use]
    pub fn permits(
        &self,
        resource_type: &str,
        operation: FhirOperation,
        patient_context: Option<&str>,
    ) -> bool {
        // Capabilities is always allowed
        if operation.always_allowed() {
            return true;
        }

        // Check if any resource scope permits the operation
        self.resource_scopes
            .iter()
            .any(|s| s.matches(resource_type, operation, patient_context))
    }

    /// Get all resource types that these scopes grant access to.
    ///
    /// Returns specific resource types; if wildcard access is present,
    /// returns an empty vec (use `has_wildcard_access()` to check).
    #[must_use]
    pub fn accessible_resource_types(&self) -> Vec<String> {
        let mut types = Vec::new();
        for scope in &self.resource_scopes {
            match &scope.resource_type {
                ResourceType::Specific(rt) => {
                    if !types.contains(rt) {
                        types.push(rt.clone());
                    }
                }
                ResourceType::Wildcard => {
                    // Wildcard access - return empty vec
                    return Vec::new();
                }
            }
        }
        types
    }

    /// Returns true if any scope grants wildcard (*) resource access.
    #[must_use]
    pub fn has_wildcard_access(&self) -> bool {
        self.resource_scopes
            .iter()
            .any(|s| matches!(s.resource_type, ResourceType::Wildcard))
    }

    /// Returns true if any scope uses system/* context.
    #[must_use]
    pub fn has_system_scopes(&self) -> bool {
        self.resource_scopes
            .iter()
            .any(|s| s.context == ScopeContext::System)
    }

    /// Validate requested scopes against allowed scopes.
    ///
    /// Returns the intersection of requested and allowed scopes, or an error
    /// if any requested scope is not covered by allowed scopes.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::SmartScopes;
    ///
    /// let allowed = SmartScopes::parse("patient/*.cruds offline_access").unwrap();
    /// let requested = SmartScopes::parse("patient/Observation.rs offline_access").unwrap();
    ///
    /// let validated = allowed.validate_against(&requested).unwrap();
    /// assert_eq!(validated.resource_scopes.len(), 1);
    /// assert!(validated.offline_access);
    /// ```
    pub fn validate_against(&self, requested: &SmartScopes) -> Result<SmartScopes, ScopeError> {
        let mut result = SmartScopes::default();

        // Validate resource scopes
        for req_scope in &requested.resource_scopes {
            if self.permits_scope(req_scope) {
                result.resource_scopes.push(req_scope.clone());
            } else {
                return Err(ScopeError::ScopeNotPermitted(req_scope.to_string()));
            }
        }

        // Validate special scopes
        if requested.launch {
            if self.launch {
                result.launch = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("launch".to_string()));
            }
        }

        if requested.launch_patient {
            if self.launch_patient {
                result.launch_patient = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("launch/patient".to_string()));
            }
        }

        if requested.launch_encounter {
            if self.launch_encounter {
                result.launch_encounter = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted(
                    "launch/encounter".to_string(),
                ));
            }
        }

        if requested.openid {
            if self.openid {
                result.openid = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("openid".to_string()));
            }
        }

        if requested.fhir_user {
            if self.fhir_user {
                result.fhir_user = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("fhirUser".to_string()));
            }
        }

        if requested.offline_access {
            if self.offline_access {
                result.offline_access = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("offline_access".to_string()));
            }
        }

        if requested.online_access {
            if self.online_access {
                result.online_access = true;
            } else {
                return Err(ScopeError::ScopeNotPermitted("online_access".to_string()));
            }
        }

        Ok(result)
    }

    /// Check if a requested scope is covered by any allowed scope.
    fn permits_scope(&self, requested: &SmartScope) -> bool {
        self.resource_scopes.iter().any(|s| s.covers(requested))
    }

    // -------------------------------------------------------------------------
    // Downscoping Methods
    // -------------------------------------------------------------------------

    /// Convert user/* scopes to patient/* scopes for a specific patient.
    ///
    /// This is used when a user-level access needs to be restricted to
    /// a specific patient context (e.g., during EHR launch).
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::{SmartScopes, ScopeContext};
    ///
    /// let scopes = SmartScopes::parse("user/Observation.rs user/Patient.r").unwrap();
    /// let patient_scopes = scopes.downscope_to_patient("patient-123");
    ///
    /// // All scopes are now patient/* context
    /// assert!(patient_scopes.resource_scopes.iter().all(|s| s.context == ScopeContext::Patient));
    /// ```
    #[must_use]
    pub fn downscope_to_patient(&self, _patient_id: &str) -> Self {
        let mut result = self.clone();

        for scope in &mut result.resource_scopes {
            // Convert user/* scopes to patient/* scopes
            if scope.context == ScopeContext::User {
                scope.context = ScopeContext::Patient;
            }
        }

        result
    }

    /// Compute the intersection of two scope sets.
    ///
    /// Returns a new scope set containing only the scopes that are present in both sets.
    /// For resource scopes, finds the common subset of permissions.
    ///
    /// # Examples
    ///
    /// ```
    /// use octofhir_auth::smart::scopes::SmartScopes;
    ///
    /// let a = SmartScopes::parse("patient/Observation.crus offline_access").unwrap();
    /// let b = SmartScopes::parse("patient/Observation.rs patient/Patient.r online_access").unwrap();
    ///
    /// let intersection = a.intersect(&b);
    /// assert_eq!(intersection.resource_scopes.len(), 1);
    /// // Only Observation with rs (common permissions)
    /// let obs = &intersection.resource_scopes[0];
    /// assert!(!obs.permissions.create);  // only in a
    /// assert!(obs.permissions.read);      // in both
    /// assert!(!obs.permissions.update);   // only in a
    /// assert!(obs.permissions.search);    // in both
    /// ```
    #[must_use]
    pub fn intersect(&self, other: &SmartScopes) -> SmartScopes {
        let mut result = SmartScopes::default();

        // Intersect resource scopes
        for self_scope in &self.resource_scopes {
            for other_scope in &other.resource_scopes {
                if let Some(intersection) = self_scope.intersect(other_scope) {
                    // Avoid duplicates
                    if !result.resource_scopes.iter().any(|s| {
                        s.context == intersection.context
                            && s.resource_type == intersection.resource_type
                    }) {
                        result.resource_scopes.push(intersection);
                    }
                }
            }
        }

        // Intersect special scopes
        result.launch = self.launch && other.launch;
        result.launch_patient = self.launch_patient && other.launch_patient;
        result.launch_encounter = self.launch_encounter && other.launch_encounter;
        result.openid = self.openid && other.openid;
        result.fhir_user = self.fhir_user && other.fhir_user;
        result.offline_access = self.offline_access && other.offline_access;
        result.online_access = self.online_access && other.online_access;

        result
    }
}

impl fmt::Display for SmartScopes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if self.launch {
            parts.push("launch".to_string());
        }
        if self.launch_patient {
            parts.push("launch/patient".to_string());
        }
        if self.launch_encounter {
            parts.push("launch/encounter".to_string());
        }
        if self.openid {
            parts.push("openid".to_string());
        }
        if self.fhir_user {
            parts.push("fhirUser".to_string());
        }
        if self.offline_access {
            parts.push("offline_access".to_string());
        }
        if self.online_access {
            parts.push("online_access".to_string());
        }

        for scope in &self.resource_scopes {
            parts.push(scope.to_string());
        }

        write!(f, "{}", parts.join(" "))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Permissions Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_permissions() {
        let perms = Permissions::from_str("crus").unwrap();
        assert!(perms.create);
        assert!(perms.read);
        assert!(perms.update);
        assert!(!perms.delete);
        assert!(perms.search);
    }

    #[test]
    fn test_parse_permissions_all() {
        let perms = Permissions::from_str("cruds").unwrap();
        assert!(perms.create);
        assert!(perms.read);
        assert!(perms.update);
        assert!(perms.delete);
        assert!(perms.search);
        assert!(perms.is_full());
    }

    #[test]
    fn test_parse_permissions_read_only() {
        let perms = Permissions::from_str("r").unwrap();
        assert!(!perms.create);
        assert!(perms.read);
        assert!(!perms.update);
        assert!(!perms.delete);
        assert!(!perms.search);
        assert!(perms.can_read());
        assert!(!perms.can_write());
    }

    #[test]
    fn test_parse_permissions_order_error() {
        assert!(Permissions::from_str("rc").is_err());
        assert!(Permissions::from_str("sr").is_err());
        assert!(Permissions::from_str("dc").is_err());
    }

    #[test]
    fn test_parse_permissions_invalid_char() {
        let err = Permissions::from_str("rx").unwrap_err();
        assert!(matches!(err, ScopeError::InvalidPermission('x')));
    }

    #[test]
    fn test_parse_permissions_duplicate() {
        // Duplicates violate ordering
        assert!(Permissions::from_str("rr").is_err());
    }

    #[test]
    fn test_permissions_display() {
        let perms = Permissions::from_str("crs").unwrap();
        assert_eq!(perms.to_string(), "crs");
    }

    #[test]
    fn test_permissions_can_write() {
        let perms = Permissions::from_str("cud").unwrap();
        assert!(perms.can_write());

        let perms = Permissions::from_str("rs").unwrap();
        assert!(!perms.can_write());
    }

    // -------------------------------------------------------------------------
    // SmartScope Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_resource_scope() {
        let scope = SmartScope::parse("patient/Observation.rs").unwrap();
        assert_eq!(scope.context, ScopeContext::Patient);
        assert!(matches!(
            scope.resource_type,
            ResourceType::Specific(ref s) if s == "Observation"
        ));
        assert!(scope.permissions.read);
        assert!(scope.permissions.search);
        assert!(!scope.permissions.create);
        assert!(scope.filter.is_none());
    }

    #[test]
    fn test_parse_wildcard_scope() {
        let scope = SmartScope::parse("system/*.cruds").unwrap();
        assert_eq!(scope.context, ScopeContext::System);
        assert_eq!(scope.resource_type, ResourceType::Wildcard);
        assert!(scope.permissions.is_full());
    }

    #[test]
    fn test_parse_user_scope() {
        let scope = SmartScope::parse("user/Patient.r").unwrap();
        assert_eq!(scope.context, ScopeContext::User);
        assert!(matches!(
            scope.resource_type,
            ResourceType::Specific(ref s) if s == "Patient"
        ));
    }

    #[test]
    fn test_parse_scope_with_filter() {
        let scope = SmartScope::parse("patient/Observation.rs?category=laboratory").unwrap();
        let filter = scope.filter.as_ref().unwrap();
        assert_eq!(filter.parameter, "category");
        assert_eq!(filter.value, "laboratory");
    }

    #[test]
    fn test_parse_scope_with_empty_filter_value() {
        let scope = SmartScope::parse("patient/Observation.rs?category=").unwrap();
        let filter = scope.filter.as_ref().unwrap();
        assert_eq!(filter.parameter, "category");
        assert_eq!(filter.value, "");
    }

    #[test]
    fn test_parse_scope_invalid_context() {
        let err = SmartScope::parse("admin/Patient.r").unwrap_err();
        assert!(matches!(err, ScopeError::InvalidContext(_)));
    }

    #[test]
    fn test_parse_scope_invalid_format_no_slash() {
        let err = SmartScope::parse("patientPatient.r").unwrap_err();
        assert!(matches!(err, ScopeError::InvalidFormat(_)));
    }

    #[test]
    fn test_parse_scope_invalid_format_no_dot() {
        let err = SmartScope::parse("patient/Patientr").unwrap_err();
        assert!(matches!(err, ScopeError::InvalidFormat(_)));
    }

    #[test]
    fn test_smart_scope_display() {
        let scope = SmartScope::parse("patient/Observation.rs?category=laboratory").unwrap();
        assert_eq!(
            scope.to_string(),
            "patient/Observation.rs?category=laboratory"
        );
    }

    #[test]
    fn test_smart_scope_matches_resource() {
        let scope = SmartScope::parse("patient/Observation.r").unwrap();
        assert!(scope.matches_resource("Observation"));
        assert!(!scope.matches_resource("Patient"));

        let wildcard = SmartScope::parse("system/*.r").unwrap();
        assert!(wildcard.matches_resource("Observation"));
        assert!(wildcard.matches_resource("Patient"));
    }

    // -------------------------------------------------------------------------
    // SmartScopes Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_launch_scopes() {
        let scopes = SmartScopes::parse("launch launch/patient openid fhirUser").unwrap();
        assert!(scopes.launch);
        assert!(scopes.launch_patient);
        assert!(!scopes.launch_encounter);
        assert!(scopes.openid);
        assert!(scopes.fhir_user);
    }

    #[test]
    fn test_parse_access_scopes() {
        let scopes = SmartScopes::parse("offline_access").unwrap();
        assert!(scopes.offline_access);
        assert!(!scopes.online_access);
        assert!(scopes.wants_refresh_token());

        let scopes = SmartScopes::parse("online_access").unwrap();
        assert!(!scopes.offline_access);
        assert!(scopes.online_access);
        assert!(scopes.wants_refresh_token());
    }

    #[test]
    fn test_parse_full_scope_string() {
        let scope_str =
            "launch openid fhirUser patient/Patient.r patient/Observation.rs offline_access";
        let scopes = SmartScopes::parse(scope_str).unwrap();
        assert!(scopes.launch);
        assert!(scopes.openid);
        assert!(scopes.fhir_user);
        assert!(scopes.offline_access);
        assert_eq!(scopes.resource_scopes.len(), 2);
    }

    #[test]
    fn test_parse_scopes_ignores_unknown() {
        // Unknown scopes should be silently ignored per SMART spec
        let scopes = SmartScopes::parse("launch unknown_scope openid").unwrap();
        assert!(scopes.launch);
        assert!(scopes.openid);
    }

    #[test]
    fn test_parse_empty_scope_string() {
        let scopes = SmartScopes::parse("").unwrap();
        assert!(scopes.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let scopes = SmartScopes::parse("   \t  \n  ").unwrap();
        assert!(scopes.is_empty());
    }

    #[test]
    fn test_smart_scopes_display() {
        let scopes = SmartScopes::parse("launch openid patient/Observation.rs").unwrap();
        let output = scopes.to_string();
        assert!(output.contains("launch"));
        assert!(output.contains("openid"));
        assert!(output.contains("patient/Observation.rs"));
    }

    #[test]
    fn test_smart_scopes_resource_access() {
        let scopes = SmartScopes::parse("patient/Observation.r patient/Patient.cru").unwrap();

        assert!(scopes.has_resource_access("Observation"));
        assert!(scopes.has_resource_access("Patient"));
        assert!(!scopes.has_resource_access("Condition"));

        assert!(scopes.can_read_resource("Observation"));
        assert!(scopes.can_read_resource("Patient"));

        assert!(!scopes.can_write_resource("Observation"));
        assert!(scopes.can_write_resource("Patient"));
    }

    #[test]
    fn test_smart_scopes_wildcard_access() {
        let scopes = SmartScopes::parse("system/*.cruds").unwrap();
        assert!(scopes.has_resource_access("Observation"));
        assert!(scopes.has_resource_access("Patient"));
        assert!(scopes.can_read_resource("Anything"));
        assert!(scopes.can_write_resource("Anything"));
    }

    #[test]
    fn test_scopes_for_context() {
        let scopes =
            SmartScopes::parse("patient/Observation.r user/Patient.r system/*.cruds").unwrap();

        let patient_scopes: Vec<_> = scopes.scopes_for_context(ScopeContext::Patient).collect();
        assert_eq!(patient_scopes.len(), 1);

        let user_scopes: Vec<_> = scopes.scopes_for_context(ScopeContext::User).collect();
        assert_eq!(user_scopes.len(), 1);

        let system_scopes: Vec<_> = scopes.scopes_for_context(ScopeContext::System).collect();
        assert_eq!(system_scopes.len(), 1);
    }

    // -------------------------------------------------------------------------
    // Roundtrip Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_scope_roundtrip() {
        let original = "patient/Observation.rs?category=laboratory";
        let scope = SmartScope::parse(original).unwrap();
        assert_eq!(scope.to_string(), original);
    }

    #[test]
    fn test_permissions_roundtrip() {
        for perms_str in ["r", "rs", "cru", "cruds", "cds"] {
            let perms = Permissions::from_str(perms_str).unwrap();
            assert_eq!(perms.to_string(), perms_str);
        }
    }

    // -------------------------------------------------------------------------
    // FhirOperation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fhir_operation_required_permission() {
        assert_eq!(FhirOperation::Read.required_permission(), Some('r'));
        assert_eq!(FhirOperation::VRead.required_permission(), Some('r'));
        assert_eq!(FhirOperation::Create.required_permission(), Some('c'));
        assert_eq!(FhirOperation::Update.required_permission(), Some('u'));
        assert_eq!(FhirOperation::Patch.required_permission(), Some('u'));
        assert_eq!(FhirOperation::Delete.required_permission(), Some('d'));
        assert_eq!(FhirOperation::Search.required_permission(), Some('s'));
        assert_eq!(FhirOperation::SearchType.required_permission(), Some('s'));
        assert_eq!(
            FhirOperation::HistoryInstance.required_permission(),
            Some('r')
        );
        assert_eq!(FhirOperation::Capabilities.required_permission(), None);
        assert_eq!(FhirOperation::Batch.required_permission(), None);
    }

    #[test]
    fn test_fhir_operation_always_allowed() {
        assert!(FhirOperation::Capabilities.always_allowed());
        assert!(!FhirOperation::Read.always_allowed());
        assert!(!FhirOperation::Search.always_allowed());
    }

    #[test]
    fn test_fhir_operation_levels() {
        // Instance level
        assert!(FhirOperation::Read.is_instance_level());
        assert!(FhirOperation::VRead.is_instance_level());
        assert!(FhirOperation::Update.is_instance_level());
        assert!(!FhirOperation::Create.is_instance_level());
        assert!(!FhirOperation::Search.is_instance_level());

        // Type level
        assert!(FhirOperation::Create.is_type_level());
        assert!(FhirOperation::Search.is_type_level());
        assert!(!FhirOperation::Read.is_type_level());

        // System level
        assert!(FhirOperation::Capabilities.is_system_level());
        assert!(FhirOperation::Batch.is_system_level());
        assert!(FhirOperation::SearchSystem.is_system_level());
        assert!(!FhirOperation::Read.is_system_level());
    }

    // -------------------------------------------------------------------------
    // Scope Validation Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_permits_read() {
        let scopes = SmartScopes::parse("patient/Observation.rs").unwrap();

        // Should permit read with patient context
        assert!(scopes.permits("Observation", FhirOperation::Read, Some("patient-123")));

        // Should permit search with patient context
        assert!(scopes.permits("Observation", FhirOperation::Search, Some("patient-123")));

        // Should not permit create (no 'c' permission)
        assert!(!scopes.permits("Observation", FhirOperation::Create, Some("patient-123")));

        // Should not permit different resource type
        assert!(!scopes.permits("Patient", FhirOperation::Read, Some("patient-123")));
    }

    #[test]
    fn test_permits_wildcard() {
        let scopes = SmartScopes::parse("system/*.cruds").unwrap();

        // Should permit any operation on any resource
        assert!(scopes.permits("Observation", FhirOperation::Read, None));
        assert!(scopes.permits("Patient", FhirOperation::Create, None));
        assert!(scopes.permits("Condition", FhirOperation::Delete, None));
    }

    #[test]
    fn test_patient_context_required() {
        let scopes = SmartScopes::parse("patient/Observation.rs").unwrap();

        // Without patient context, patient/* scopes should not permit
        assert!(!scopes.permits("Observation", FhirOperation::Read, None));

        // With patient context, should permit
        assert!(scopes.permits("Observation", FhirOperation::Read, Some("patient-123")));
    }

    #[test]
    fn test_system_scope_no_patient_required() {
        let scopes = SmartScopes::parse("system/Observation.rs").unwrap();

        // System scopes don't require patient context
        assert!(scopes.permits("Observation", FhirOperation::Read, None));
    }

    #[test]
    fn test_capabilities_always_allowed() {
        // Even with empty scopes, capabilities should be allowed
        let scopes = SmartScopes::default();
        assert!(scopes.permits("metadata", FhirOperation::Capabilities, None));
    }

    #[test]
    fn test_scope_validation() {
        let allowed = SmartScopes::parse("patient/*.cruds offline_access openid").unwrap();
        let requested = SmartScopes::parse("patient/Observation.rs offline_access").unwrap();

        let validated = allowed.validate_against(&requested).unwrap();
        assert_eq!(validated.resource_scopes.len(), 1);
        assert!(validated.offline_access);
        assert!(!validated.openid); // Not requested
    }

    #[test]
    fn test_scope_validation_denied() {
        let allowed = SmartScopes::parse("patient/Patient.r").unwrap();
        let requested = SmartScopes::parse("patient/Observation.rs").unwrap();

        let result = allowed.validate_against(&requested);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ScopeError::ScopeNotPermitted(_)
        ));
    }

    #[test]
    fn test_scope_downscope_to_patient() {
        let scopes = SmartScopes::parse("user/Observation.rs user/Patient.r").unwrap();
        let downscoped = scopes.downscope_to_patient("patient-123");

        // All scopes should now be patient/* context
        for scope in &downscoped.resource_scopes {
            assert_eq!(scope.context, ScopeContext::Patient);
        }
    }

    #[test]
    fn test_scope_intersect() {
        let a = SmartScopes::parse("patient/Observation.cruds offline_access").unwrap();
        let b = SmartScopes::parse("patient/Observation.rs online_access").unwrap();

        let intersection = a.intersect(&b);

        // Should have intersection of permissions (only r and s)
        assert_eq!(intersection.resource_scopes.len(), 1);
        let scope = &intersection.resource_scopes[0];
        assert!(!scope.permissions.create);
        assert!(scope.permissions.read);
        assert!(!scope.permissions.update);
        assert!(!scope.permissions.delete);
        assert!(scope.permissions.search);

        // Special scopes: neither offline nor online (not in both)
        assert!(!intersection.offline_access);
        assert!(!intersection.online_access);
    }

    #[test]
    fn test_accessible_resource_types() {
        let scopes = SmartScopes::parse("patient/Observation.r patient/Patient.r").unwrap();
        let types = scopes.accessible_resource_types();

        assert_eq!(types.len(), 2);
        assert!(types.contains(&"Observation".to_string()));
        assert!(types.contains(&"Patient".to_string()));
    }

    #[test]
    fn test_accessible_resource_types_wildcard() {
        let scopes = SmartScopes::parse("patient/*.r").unwrap();
        let types = scopes.accessible_resource_types();

        // Wildcard returns empty vec
        assert!(types.is_empty());
        assert!(scopes.has_wildcard_access());
    }

    #[test]
    fn test_has_system_scopes() {
        let patient_scopes = SmartScopes::parse("patient/Observation.r").unwrap();
        assert!(!patient_scopes.has_system_scopes());

        let system_scopes = SmartScopes::parse("system/Observation.r").unwrap();
        assert!(system_scopes.has_system_scopes());
    }

    #[test]
    fn test_smart_scope_covers() {
        let wildcard = SmartScope::parse("patient/*.cruds").unwrap();
        let specific = SmartScope::parse("patient/Observation.rs").unwrap();

        // Wildcard covers specific
        assert!(wildcard.covers(&specific));

        // Specific doesn't cover wildcard
        assert!(!specific.covers(&wildcard));
    }

    #[test]
    fn test_smart_scope_covers_permissions() {
        let full = SmartScope::parse("patient/Observation.cruds").unwrap();
        let partial = SmartScope::parse("patient/Observation.rs").unwrap();

        // Full permissions cover partial
        assert!(full.covers(&partial));

        // Partial doesn't cover full
        assert!(!partial.covers(&full));
    }

    #[test]
    fn test_smart_scope_intersect() {
        let a = SmartScope::parse("patient/Observation.cruds").unwrap();
        let b = SmartScope::parse("patient/Observation.rs").unwrap();

        let intersection = a.intersect(&b).unwrap();
        assert!(intersection.permissions.read);
        assert!(intersection.permissions.search);
        assert!(!intersection.permissions.create);
        assert!(!intersection.permissions.update);
        assert!(!intersection.permissions.delete);
    }

    #[test]
    fn test_smart_scope_intersect_no_overlap() {
        let a = SmartScope::parse("patient/Observation.r").unwrap();
        let b = SmartScope::parse("patient/Patient.r").unwrap();

        // Different resource types - no intersection
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn test_smart_scope_intersect_no_permissions() {
        let a = SmartScope::parse("patient/Observation.c").unwrap();
        let b = SmartScope::parse("patient/Observation.r").unwrap();

        // No overlapping permissions - no intersection
        assert!(a.intersect(&b).is_none());
    }

    // -------------------------------------------------------------------------
    // Standalone Context Requirements Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_context_requirements_patient_only() {
        let scopes = SmartScopes::parse("launch/patient patient/Patient.rs openid").unwrap();
        let reqs = scopes.context_requirements();

        assert!(reqs.needs_patient_selection);
        assert!(!reqs.needs_encounter_selection);
        assert!(reqs.needs_any_selection());
    }

    #[test]
    fn test_context_requirements_encounter_only() {
        let scopes = SmartScopes::parse("launch/encounter patient/Encounter.rs").unwrap();
        let reqs = scopes.context_requirements();

        assert!(!reqs.needs_patient_selection);
        assert!(reqs.needs_encounter_selection);
        assert!(reqs.needs_any_selection());
    }

    #[test]
    fn test_context_requirements_both() {
        let scopes =
            SmartScopes::parse("launch/patient launch/encounter patient/Patient.rs").unwrap();
        let reqs = scopes.context_requirements();

        assert!(reqs.needs_patient_selection);
        assert!(reqs.needs_encounter_selection);
        assert!(reqs.needs_any_selection());
    }

    #[test]
    fn test_context_requirements_none() {
        let scopes = SmartScopes::parse("patient/Patient.rs openid").unwrap();
        let reqs = scopes.context_requirements();

        assert!(!reqs.needs_patient_selection);
        assert!(!reqs.needs_encounter_selection);
        assert!(!reqs.needs_any_selection());
    }

    #[test]
    fn test_is_standalone_with_context() {
        // Standalone with patient context
        let scopes = SmartScopes::parse("launch/patient patient/Patient.rs").unwrap();
        assert!(scopes.is_standalone_with_context());

        // EHR launch with patient context (has 'launch' scope)
        let scopes = SmartScopes::parse("launch launch/patient patient/Patient.rs").unwrap();
        assert!(!scopes.is_standalone_with_context());

        // Standalone without context
        let scopes = SmartScopes::parse("patient/Patient.rs").unwrap();
        assert!(!scopes.is_standalone_with_context());
    }
}
