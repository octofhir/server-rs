//! Database-backed object storage providers independent of clinical models and IGs.
//!
//! # Contents
//! Administrator resource API, create-only deployment seeds, S3 connections and
//! encrypted credentials resolved on demand by package acquisition.
//!
//! # Invariants
//! Database edits survive restart. Credentials never serialize into public metadata
//! or audit history. Provider schema is available before canonical packages load.
pub mod api;
pub(crate) mod model;
pub(crate) mod store;
pub use model::{AddressingStyle, ProviderCredentials, ProviderMeta, StorageProvider};
