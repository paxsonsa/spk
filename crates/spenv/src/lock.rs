// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Lock file structures and helpers for spenv.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{ComposedEnvironment, EnvSpec};

#[cfg(test)]
#[path = "./lock_test.rs"]
mod lock_test;

/// Lock file API version.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum LockApiVersion {
    #[serde(rename = "spenv/v0/lock")]
    V0,
}

/// Lock file structure capturing sources and resolved layers.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LockFile {
    pub api: LockApiVersion,
    pub generated: GenerationMetadata,
    pub sources: Vec<SourceFile>,
    pub layers: Vec<ResolvedLayer>,
}

/// Metadata about when and where the lock was generated.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct GenerationMetadata {
    pub timestamp: DateTime<Utc>,
    pub spenv_version: String,
    pub hostname: String,
}

/// Source spec file tracked by the lock.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SourceFile {
    pub path: PathBuf,
    pub sha256: String,
    pub mtime: DateTime<Utc>,
}

/// Resolved layer in the locked environment.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResolvedLayer {
    pub reference: String,
    pub digest: String,
    pub resolved_at: DateTime<Utc>,
}

/// Generate lock file from composed environment.
pub async fn generate_lock(
    _specs: &[EnvSpec],
    composed: &ComposedEnvironment,
    repo: &spfs::storage::RepositoryHandle,
) -> crate::Result<LockFile> {
    use sha2::{Digest as ShaDigest, Sha256};

    // Hash source files
    let mut sources = Vec::new();
    for path in &composed.source_files {
        let content = std::fs::read(path)?;
        let hash = Sha256::digest(&content);
        let hash_hex = format!("{:x}", hash);

        let metadata = std::fs::metadata(path)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0))
            .unwrap_or_else(Utc::now);

        sources.push(SourceFile {
            path: path.clone(),
            sha256: hash_hex,
            mtime,
        });
    }

    // Resolve layers to digests
    let mut layers = Vec::new();
    for layer_ref in &composed.layers {
        let digest = crate::runtime::resolve_layer_reference(layer_ref, repo).await?;

        layers.push(ResolvedLayer {
            reference: layer_ref.clone(),
            digest: digest.to_string(),
            resolved_at: Utc::now(),
        });
    }

    Ok(LockFile {
        api: LockApiVersion::V0,
        generated: GenerationMetadata {
            timestamp: Utc::now(),
            spenv_version: env!("CARGO_PKG_VERSION").to_string(),
            hostname: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string()),
        },
        sources,
        layers,
    })
}

/// Verify lock file matches current environment.
pub async fn verify_lock(
    lock: &LockFile,
    _specs: &[EnvSpec],
    composed: &ComposedEnvironment,
    repo: &spfs::storage::RepositoryHandle,
) -> crate::Result<Vec<LockChange>> {
    let mut changes = Vec::new();

    // Check source file hashes
    for (i, source) in lock.sources.iter().enumerate() {
        if i >= composed.source_files.len() {
            changes.push(LockChange {
                kind: LockChangeKind::SourceFileRemoved,
                reference: source.path.display().to_string(),
                expected: Some(source.sha256.clone()),
                actual: None,
            });
            continue;
        }

        let actual_path = &composed.source_files[i];
        let content = std::fs::read(actual_path)?;
        use sha2::Digest as ShaDigest;
        let actual_hash = format!("{:x}", sha2::Sha256::digest(&content));

        if actual_hash != source.sha256 {
            changes.push(LockChange {
                kind: LockChangeKind::SourceFileChanged,
                reference: source.path.display().to_string(),
                expected: Some(source.sha256.clone()),
                actual: Some(actual_hash),
            });
        }
    }

    // Check layer digests
    for (i, locked_layer) in lock.layers.iter().enumerate() {
        if i >= composed.layers.len() {
            changes.push(LockChange {
                kind: LockChangeKind::LayerRemoved,
                reference: locked_layer.reference.clone(),
                expected: Some(locked_layer.digest.clone()),
                actual: None,
            });
            continue;
        }

        let actual_ref = &composed.layers[i];
        let actual_digest = crate::runtime::resolve_layer_reference(actual_ref, repo).await?;

        if actual_digest.to_string() != locked_layer.digest {
            changes.push(LockChange {
                kind: LockChangeKind::LayerDigestChanged,
                reference: locked_layer.reference.clone(),
                expected: Some(locked_layer.digest.clone()),
                actual: Some(actual_digest.to_string()),
            });
        }
    }

    // Extra layers beyond those in the lock are reported as added.
    if composed.layers.len() > lock.layers.len() {
        for extra in composed.layers.iter().skip(lock.layers.len()) {
            changes.push(LockChange {
                kind: LockChangeKind::LayerAdded,
                reference: extra.clone(),
                expected: None,
                actual: None,
            });
        }
    }

    Ok(changes)
}

/// A single detected change between lock and current environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockChange {
    pub kind: LockChangeKind,
    pub reference: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

/// Types of lock mismatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockChangeKind {
    LayerDigestChanged,
    LayerAdded,
    LayerRemoved,
    SourceFileChanged,
    SourceFileRemoved,
}
