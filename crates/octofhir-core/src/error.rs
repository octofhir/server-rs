use thiserror::Error;

/// Core error types for OctoFHIR operations
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Invalid FHIR resource type: {0}")]
    InvalidResourceType(String),

    #[error("Invalid FHIR ID: {0}")]
    InvalidId(String),

    #[error("Invalid FHIR DateTime: {0}")]
    InvalidDateTime(String),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Time parsing error: {0}")]
    TimeError(#[from] time::error::Parse),

    #[error("UUID error: {0}")]
    UuidError(#[from] uuid::Error),

    #[error("Resource not found: {resource_type}/{id}")]
    ResourceNotFound { resource_type: String, id: String },

    #[error("Resource conflict: {resource_type}/{id} already exists")]
    ResourceConflict { resource_type: String, id: String },

    #[error("Resource deleted: {resource_type}/{id}")]
    ResourceDeleted { resource_type: String, id: String },

    #[error("Invalid resource data: {message}")]
    InvalidResource { message: String },

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("URL parsing error: {0}")]
    UrlError(#[from] url::ParseError),

    #[error("Regex error: {0}")]
    RegexError(#[from] regex::Error),
}

impl CoreError {
    /// Create a new InvalidResourceType error
    pub fn invalid_resource_type(resource_type: impl Into<String>) -> Self {
        Self::InvalidResourceType(resource_type.into())
    }

    /// Create a new InvalidId error
    pub fn invalid_id(id: impl Into<String>) -> Self {
        Self::InvalidId(id.into())
    }

    /// Create a new InvalidDateTime error
    pub fn invalid_date_time(datetime: impl Into<String>) -> Self {
        Self::InvalidDateTime(datetime.into())
    }

    /// Create a new ResourceNotFound error
    pub fn resource_not_found(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::ResourceNotFound {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Create a new ResourceConflict error
    pub fn resource_conflict(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::ResourceConflict {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Create a new ResourceDeleted error (for soft-deleted resources - 410 Gone)
    pub fn resource_deleted(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::ResourceDeleted {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Create a new InvalidResource error
    pub fn invalid_resource(message: impl Into<String>) -> Self {
        Self::InvalidResource {
            message: message.into(),
        }
    }

    /// Create a new Configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }

    /// Check if this error is a client error (4xx category)
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidResourceType(_)
                | Self::InvalidId(_)
                | Self::InvalidDateTime(_)
                | Self::InvalidResource { .. }
                | Self::ResourceNotFound { .. }
                | Self::ResourceConflict { .. }
                | Self::ResourceDeleted { .. }
                | Self::JsonError(_)
                | Self::UrlError(_)
        )
    }

    /// Check if this error is a server error (5xx category)
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            Self::Configuration(_) | Self::TimeError(_) | Self::UuidError(_) | Self::RegexError(_)
        )
    }

    /// Get error category for logging/monitoring
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidResourceType(_) | Self::InvalidId(_) | Self::InvalidDateTime(_) => {
                ErrorCategory::Validation
            }
            Self::ResourceNotFound { .. } => ErrorCategory::NotFound,
            Self::ResourceConflict { .. } => ErrorCategory::Conflict,
            Self::ResourceDeleted { .. } => ErrorCategory::Deleted,
            Self::InvalidResource { .. } => ErrorCategory::Validation,
            Self::JsonError(_) => ErrorCategory::Serialization,
            Self::TimeError(_) | Self::UuidError(_) => ErrorCategory::System,
            Self::Configuration(_) => ErrorCategory::Configuration,
            Self::UrlError(_) => ErrorCategory::Validation,
            Self::RegexError(_) => ErrorCategory::System,
        }
    }
}

/// Error categories for monitoring and classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Validation,
    NotFound,
    Conflict,
    Deleted,
    Serialization,
    System,
    Configuration,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation => write!(f, "validation"),
            Self::NotFound => write!(f, "not_found"),
            Self::Conflict => write!(f, "conflict"),
            Self::Deleted => write!(f, "deleted"),
            Self::Serialization => write!(f, "serialization"),
            Self::System => write!(f, "system"),
            Self::Configuration => write!(f, "configuration"),
        }
    }
}

/// Convenience result type for core operations
pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = CoreError::invalid_resource_type("InvalidType");
        assert_eq!(err.to_string(), "Invalid FHIR resource type: InvalidType");
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
        assert_eq!(err.category(), ErrorCategory::Validation);
    }

    #[test]
    fn test_resource_not_found_error() {
        let err = CoreError::resource_not_found("Patient", "123");
        assert_eq!(err.to_string(), "Resource not found: Patient/123");
        assert!(err.is_client_error());
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn test_resource_conflict_error() {
        let err = CoreError::resource_conflict("Patient", "456");
        assert_eq!(
            err.to_string(),
            "Resource conflict: Patient/456 already exists"
        );
        assert!(err.is_client_error());
        assert_eq!(err.category(), ErrorCategory::Conflict);
    }

    #[test]
    fn test_json_error_conversion() {
        let invalid_json = "{ invalid json }";
        let json_err: serde_json::Error =
            serde_json::from_str::<serde_json::Value>(invalid_json).unwrap_err();
        let core_err: CoreError = json_err.into();

        assert!(matches!(core_err, CoreError::JsonError(_)));
        assert!(core_err.is_client_error());
        assert_eq!(core_err.category(), ErrorCategory::Serialization);
    }

    #[test]
    fn test_configuration_error() {
        let err = CoreError::configuration("Invalid config value");
        assert_eq!(err.to_string(), "Configuration error: Invalid config value");
        assert!(err.is_server_error());
        assert_eq!(err.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn test_url_error_conversion() {
        let url_err = url::Url::parse("not a url").unwrap_err();
        let core_err: CoreError = url_err.into();

        assert!(matches!(core_err, CoreError::UrlError(_)));
        assert!(core_err.is_client_error());
        assert_eq!(core_err.category(), ErrorCategory::Validation);
    }

    #[test]
    fn test_error_categories_display() {
        assert_eq!(ErrorCategory::Validation.to_string(), "validation");
        assert_eq!(ErrorCategory::NotFound.to_string(), "not_found");
        assert_eq!(ErrorCategory::Conflict.to_string(), "conflict");
        assert_eq!(ErrorCategory::Serialization.to_string(), "serialization");
        assert_eq!(ErrorCategory::System.to_string(), "system");
        assert_eq!(ErrorCategory::Configuration.to_string(), "configuration");
    }

    #[test]
    fn test_error_debug_format() {
        let err = CoreError::invalid_resource("Test message");
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("InvalidResource"));
        assert!(debug_str.contains("Test message"));
    }

    #[test]
    fn test_client_vs_server_error_classification() {
        // Client errors
        assert!(CoreError::invalid_resource_type("Bad").is_client_error());
        assert!(CoreError::invalid_id("bad-id").is_client_error());
        assert!(CoreError::resource_not_found("Patient", "123").is_client_error());
        assert!(CoreError::resource_conflict("Patient", "123").is_client_error());

        // Server errors
        assert!(CoreError::configuration("config error").is_server_error());

        // Ensure mutual exclusivity
        let client_err = CoreError::invalid_id("test");
        assert!(client_err.is_client_error());
        assert!(!client_err.is_server_error());

        let server_err = CoreError::configuration("test");
        assert!(server_err.is_server_error());
        assert!(!server_err.is_client_error());
    }

    #[test]
    fn test_result_type_usage() {
        fn test_function() -> Result<String> {
            Ok("success".to_string())
        }

        fn test_function_error() -> Result<String> {
            Err(CoreError::invalid_id("bad"))
        }

        assert!(test_function().is_ok());
        assert!(test_function_error().is_err());
    }

    #[test]
    fn test_error_chains() {
        // Test that we can chain errors through the From trait using a parsing error
        let invalid_time_str = "25:61:61";
        match time::Time::parse(
            invalid_time_str,
            &time::format_description::parse("[hour]:[minute]:[second]").unwrap(),
        ) {
            Err(time_err) => {
                let core_err: CoreError = time_err.into();
                assert!(matches!(core_err, CoreError::TimeError(_)));
            }
            Ok(_) => panic!("Expected time parsing to fail"),
        }
    }

    #[test]
    fn test_uuid_error_conversion() {
        let uuid_str = "not-a-uuid";
        match uuid::Uuid::parse_str(uuid_str) {
            Err(uuid_err) => {
                let core_err: CoreError = uuid_err.into();
                assert!(matches!(core_err, CoreError::UuidError(_)));
                assert!(core_err.is_server_error());
                assert_eq!(core_err.category(), ErrorCategory::System);
            }
            Ok(_) => panic!("Expected UUID parsing to fail"),
        }
    }

    #[test]
    fn test_regex_error_conversion() {
        let invalid_regex = "[";
        match regex::Regex::new(invalid_regex) {
            Err(regex_err) => {
                let core_err: CoreError = regex_err.into();
                assert!(matches!(core_err, CoreError::RegexError(_)));
                assert!(core_err.is_server_error());
                assert_eq!(core_err.category(), ErrorCategory::System);
            }
            Ok(_) => panic!("Expected regex compilation to fail"),
        }
    }

    #[test]
    fn test_all_error_categories_covered() {
        // Ensure we have test coverage for all error categories
        let validation_err = CoreError::invalid_id("test");
        let not_found_err = CoreError::resource_not_found("Patient", "123");
        let conflict_err = CoreError::resource_conflict("Patient", "456");
        let serialization_err: CoreError = serde_json::from_str::<serde_json::Value>("invalid")
            .unwrap_err()
            .into();
        let system_err: CoreError = uuid::Uuid::parse_str("invalid").unwrap_err().into();
        let config_err = CoreError::configuration("test");

        assert_eq!(validation_err.category(), ErrorCategory::Validation);
        assert_eq!(not_found_err.category(), ErrorCategory::NotFound);
        assert_eq!(conflict_err.category(), ErrorCategory::Conflict);
        assert_eq!(serialization_err.category(), ErrorCategory::Serialization);
        assert_eq!(system_err.category(), ErrorCategory::System);
        assert_eq!(config_err.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn test_error_message_formats() {
        let resource_not_found = CoreError::resource_not_found("Patient", "abc-123");
        assert!(resource_not_found.to_string().contains("Patient/abc-123"));

        let resource_conflict = CoreError::resource_conflict("Observation", "def-456");
        assert!(
            resource_conflict
                .to_string()
                .contains("Observation/def-456")
        );

        let invalid_resource = CoreError::invalid_resource("Missing required field 'id'");
        assert!(
            invalid_resource
                .to_string()
                .contains("Missing required field 'id'")
        );
    }

    #[test]
    fn test_error_category_equality() {
        assert_eq!(ErrorCategory::Validation, ErrorCategory::Validation);
        assert_ne!(ErrorCategory::Validation, ErrorCategory::NotFound);
    }

    #[test]
    fn test_error_constructor_methods() {
        // Test all constructor methods work correctly
        let _ = CoreError::invalid_resource_type("Test");
        let _ = CoreError::invalid_id("test-id");
        let _ = CoreError::invalid_date_time("2023-13-45T25:61:61Z");
        let _ = CoreError::resource_not_found("Patient", "123");
        let _ = CoreError::resource_conflict("Patient", "456");
        let _ = CoreError::invalid_resource("Bad resource");
        let _ = CoreError::configuration("Bad config");
    }
}
