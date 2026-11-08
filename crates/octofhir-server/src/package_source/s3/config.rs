//! Configuration for an immutable, explicitly selected S3 package object.
//!
//! # Contents
//! Provider reference, pinned object identity, URL construction and download limits.
//!
//! # Invariants
//! Checksums are mandatory. Connection settings belong to a StorageProvider.
//! Object keys cannot contain URL-normalized dot segments.

use serde::{Deserialize, Serialize};

pub(super) const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

/// Published package archive in S3 or an S3-compatible service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct S3PackageSource {
    /// Id of an enabled database StorageProvider.
    pub provider: String,
    pub bucket: String,
    pub key: String,
    /// Required SHA-256 of the exact object bytes, expressed as 64 hexadecimal digits.
    pub sha256: String,
    pub version_id: Option<String>,
    pub timeout_ms: u64,
    pub max_archive_bytes: u64,
}

impl Default for S3PackageSource {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            key: String::new(),
            provider: String::new(),
            sha256: String::new(),
            version_id: None,
            timeout_ms: 60_000,
            max_archive_bytes: MAX_ARCHIVE_BYTES,
        }
    }
}

impl S3PackageSource {
    pub fn validate(&self) -> Result<(), String> {
        crate::storage_providers::model::validate_id(&self.provider)?;
        if !(3..=63).contains(&self.bucket.len())
            || !self
                .bucket
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
            || !self.bucket.as_bytes()[0].is_ascii_alphanumeric()
            || !self.bucket.as_bytes()[self.bucket.len() - 1].is_ascii_alphanumeric()
            || self.bucket.contains("..")
        {
            return Err("S3 package bucket must be a valid bucket name".into());
        }
        if self.key.is_empty()
            || self.key.len() > 1024
            || self.key.chars().any(char::is_control)
            || self.key.contains('\\')
            || self.key.split('/').any(|p| matches!(p, "" | "." | ".."))
        {
            return Err("S3 package key must contain nonempty object path segments without dot segments or control characters".into());
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("S3 package sha256 must contain exactly 64 hexadecimal digits".into());
        }
        if self
            .version_id
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 1024 || v.chars().any(char::is_control))
        {
            return Err("S3 package version_id is invalid".into());
        }
        if self.timeout_ms == 0
            || self.timeout_ms > 300_000
            || self.max_archive_bytes == 0
            || self.max_archive_bytes > MAX_ARCHIVE_BYTES
        {
            return Err(
                "S3 package timeout must be 1..300000 ms and archive limit 1..256 MiB".into(),
            );
        }
        Ok(())
    }

    pub(super) fn object_url(
        &self,
        provider: &crate::storage_providers::StorageProvider,
    ) -> Result<url::Url, String> {
        self.validate()?;
        if provider.id != self.provider {
            return Err("S3 source resolved a different storage provider".into());
        }
        provider.object_url(&self.bucket, &self.key, self.version_id.as_deref())
    }

    pub(crate) fn origin(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }
}
