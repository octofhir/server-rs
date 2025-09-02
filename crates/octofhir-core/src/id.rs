// ID handling - placeholder for now
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdError {
    #[error("Invalid ID")]
    Invalid,
}

pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn validate_id(_id: &str) -> Result<(), IdError> {
    Ok(()) // placeholder
}