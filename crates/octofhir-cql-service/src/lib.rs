//! CQL (Clinical Quality Language) service for OctoFHIR
//!
//! This crate provides CQL evaluation capabilities for the OctoFHIR server,
//! including:
//! - Ad-hoc CQL expression evaluation
//! - Library resource management with compilation caching
//! - Clinical quality measure evaluation
//! - FHIR data retrieval with proper authorization

pub mod config;
pub mod data_provider;
pub mod error;
pub mod library_cache;
pub mod service;
pub mod terminology_provider;

pub use config::CqlConfig;
pub use error::{CqlError, CqlResult};
pub use service::CqlService;
