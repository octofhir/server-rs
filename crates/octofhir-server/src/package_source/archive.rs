//! Bounded extraction of published FHIR package archives into private directories.
//!
//! # Contents
//! Gzip/tar limits, portable path checks and regular-file extraction.
//!
//! # Invariants
//! Never unpack links, devices, sparse files or escaping paths. Archive permissions
//! and owners are ignored. A failed extraction removes its temporary directory.

use super::{MAX_ENTRIES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(super) fn extract(path: &Path) -> Result<tempfile::TempDir, String> {
    extract_inner(path)
        .map_err(|error| format!("Cannot extract package {}: {error}", path.display()))
}

fn extract_inner(path: &Path) -> Result<tempfile::TempDir, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 256 * 1024 * 1024 {
        return Err("compressed archive exceeds 256 MiB".into());
    }
    let workspace = tempfile::Builder::new()
        .prefix("octofhir-package-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    // Also bound tar headers, padding and extended metadata, not only file sizes.
    let decoder = flate2::read::MultiGzDecoder::new(file);
    let mut input = decoder.take(MAX_TOTAL_BYTES + 1);
    {
        let mut archive = tar::Archive::new(&mut input);
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        for (count, entry) in archive.entries().map_err(|e| e.to_string())?.enumerate() {
            if count >= MAX_ENTRIES {
                return Err("archive has too many entries".into());
            }
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = safe_path(&entry.path().map_err(|e| e.to_string())?)?;
            if !seen.insert(path.clone()) {
                return Err("archive contains duplicate paths".into());
            }
            let kind = entry.header().entry_type();
            if !(kind.is_file() || kind.is_dir()) {
                return Err("archive contains a link or non-regular entry".into());
            }
            let bytes = entry.size();
            total = total.saturating_add(bytes);
            if bytes > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
                return Err("archive exceeds file or total size limits".into());
            }
            let target = workspace.path().join(&path);
            if kind.is_dir() {
                if bytes != 0 {
                    return Err("archive directory has a nonempty body".into());
                }
                std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map_err(|e| e.to_string())?;
                let copied = std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
                if copied != bytes {
                    return Err("truncated archive entry".into());
                }
                output.flush().map_err(|e| e.to_string())?;
            }
        }
    }
    // Consume the gzip trailer too; a valid tar end marker must not conceal a
    // truncated/corrupt gzip member or an oversized tail.
    std::io::copy(&mut input, &mut std::io::sink()).map_err(|e| e.to_string())?;
    if input.limit() == 0 {
        return Err("expanded archive exceeds 1 GiB".into());
    }
    Ok(workspace)
}

fn safe_path(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("archive path must be UTF-8")?;
    if text.contains(['\\', ':']) || text.len() > 4096 {
        return Err("archive contains an unsafe path".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            _ => return Err("archive contains an escaping path".into()),
        }
    }
    if result.as_os_str().is_empty() || result.components().count() > 64 {
        return Err("archive path is empty or too deep".into());
    }
    Ok(result)
}
