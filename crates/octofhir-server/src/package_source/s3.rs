//! Acquire one pinned S3 object through guarded, signed, bounded HTTP requests.
//!
//! # Contents
//! Source configuration, environment credential references, AWS SigV4 signing,
//! streaming SHA-256 verification and atomic content-addressed archive caching.
//!
//! # Invariants
//! Only configured objects are fetched; redirects, proxies and decompression are
//! disabled. No archive is exposed before its checksum passes. Response bodies,
//! credentials and signed headers are never included in diagnostics. The normal
//! local package path performs extraction, manifest/release checks and installation.

mod config;
mod signing;
#[cfg(test)]
mod tests;

use crate::egress::EgressPolicy;
use aws_credential_types::Credentials;
pub use config::S3PackageSource;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn download(
    source: &S3PackageSource,
    provider: &crate::storage_providers::store::ResolvedProvider,
    cache: &Path,
    policy: &Arc<EgressPolicy>,
) -> Result<PathBuf, String> {
    download_with(source, &provider.provider, cache, policy, || {
        provider.credentials()
    })
    .await
}

async fn download_with<F, Fut>(
    source: &S3PackageSource,
    provider: &crate::storage_providers::StorageProvider,
    cache: &Path,
    policy: &Arc<EgressPolicy>,
    resolve_credentials: F,
) -> Result<PathBuf, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Credentials, String>>,
{
    source.validate()?;
    provider.validate(policy)?;
    if !provider.enabled {
        return Err("Storage provider is disabled".into());
    }
    policy.check_url(&source.object_url(provider)?)?;
    let directory = cache.join("sources");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|_| "Cannot create S3 package cache")?;
    let target = directory.join(format!("{}.tgz", source.sha256.to_ascii_lowercase()));
    if tokio::fs::try_exists(&target)
        .await
        .map_err(|_| "Cannot inspect S3 package cache")?
    {
        if verify_cached(&target, source).await? {
            return Ok(target);
        }
        tokio::fs::remove_file(&target)
            .await
            .map_err(|_| "Cannot remove corrupt S3 package cache entry")?;
    }
    let credentials = resolve_credentials().await?;
    fetch(source, provider, &directory, &target, policy, &credentials).await?;
    Ok(target)
}

async fn verify_cached(path: &Path, source: &S3PackageSource) -> Result<bool, String> {
    let mut input = tokio::fs::File::open(path)
        .await
        .map_err(|_| "Cannot read S3 package cache entry")?;
    if input
        .metadata()
        .await
        .map_err(|_| "Cannot inspect S3 package cache entry")?
        .len()
        > source.max_archive_bytes
    {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let size = input
            .read(&mut buffer)
            .await
            .map_err(|_| "Cannot verify S3 package cache entry")?;
        if size == 0 {
            break;
        }
        bytes += size as u64;
        if bytes > source.max_archive_bytes {
            return Ok(false);
        }
        hash.update(&buffer[..size]);
    }
    Ok(hex::encode(hash.finalize()).eq_ignore_ascii_case(&source.sha256))
}

async fn fetch(
    source: &S3PackageSource,
    provider: &crate::storage_providers::StorageProvider,
    directory: &Path,
    target: &Path,
    policy: &Arc<EgressPolicy>,
    credentials: &Credentials,
) -> Result<(), String> {
    let client = policy
        .client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .timeout(Duration::from_millis(source.timeout_ms))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "Cannot create S3 package HTTP client")?;
    let url = source.object_url(provider)?;
    policy.check_url(&url)?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .build()
        .map_err(|_| "Cannot build S3 package request")?;
    signing::sign(&mut request, credentials, &provider.region)?;
    let mut response = client
        .execute(request)
        .await
        .map_err(|_| "S3 package request failed (network, TLS, timeout or egress policy)")?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!(
            "S3 package GET failed with HTTP {}",
            response.status().as_u16()
        ));
    }
    if let Some(version) = &source.version_id
        && response
            .headers()
            .get("x-amz-version-id")
            .and_then(|v| v.to_str().ok())
            != Some(version)
    {
        return Err("S3 response version does not match configured version_id".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > source.max_archive_bytes)
    {
        return Err("S3 package archive exceeds compressed size limit".into());
    }
    let temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "Cannot create temporary S3 archive")?;
    let mut output = tokio::fs::File::from_std(
        temporary
            .reopen()
            .map_err(|_| "Cannot open temporary S3 archive")?,
    );
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "S3 package body read failed")?
    {
        bytes = bytes.saturating_add(chunk.len() as u64);
        if bytes > source.max_archive_bytes {
            return Err("S3 package archive exceeds compressed size limit".into());
        }
        hash.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(|_| "Cannot write temporary S3 archive")?;
    }
    if !hex::encode(hash.finalize()).eq_ignore_ascii_case(&source.sha256) {
        return Err("S3 package SHA-256 mismatch".into());
    }
    output
        .flush()
        .await
        .map_err(|_| "Cannot flush S3 archive")?;
    drop(output);
    // Every writer publishes only completely verified content for this digest.
    let target = target.to_owned();
    tokio::task::spawn_blocking(move || {
        temporary
            .persist(target)
            .map(|_| ())
            .map_err(|_| "Cannot publish verified S3 archive")
    })
    .await
    .map_err(|_| "S3 package cache publication failed")??;
    Ok(())
}
