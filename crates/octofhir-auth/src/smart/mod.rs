//! SMART on FHIR implementation.
//!
//! This module provides SMART on FHIR specific functionality:
//!
//! - SMART scopes parsing and validation
//! - Launch context handling (EHR launch, standalone launch)
//! - SMART configuration endpoint (/.well-known/smart-configuration)
//! - Clinical scope enforcement
//! - Patient context selection
