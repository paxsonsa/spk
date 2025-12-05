// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Runtime creation for SPFS environments.

use spfs::prelude::*;
use spfs::runtime::Runtime;
use spfs::storage::RepositoryHandle;
use spfs::tracking::TagSpec;
use tempfile::TempDir;

use crate::ComposedEnvironment;
use crate::environment::{generate_startup_script, get_priority};
use crate::repository::RepoSelection;

/// Location in the runtime filesystem where startup scripts are sourced from.
const STARTUP_FILES_LOCATION: &str = "/spfs/etc/spfs/startup.d";

#[cfg(test)]
#[path = "./runtime_test.rs"]
mod runtime_test;

/// Options for runtime creation.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// Optional name for the runtime.
    pub name: Option<String>,
    /// Keep runtime after exit (durable).
    pub keep: bool,
    /// Allow writes to /spfs (editable).
    pub editable: bool,
    /// Repository selection flags (mirrors spk).
    pub repo_selection: RepoSelection,
}

/// Create SPFS runtime from composed environment.
pub async fn create_runtime(
    composed: &ComposedEnvironment,
    config: &spfs::Config,
    options: &RuntimeOptions,
) -> crate::Result<Runtime> {
    let repo = config.get_local_repository_handle().await?;
    let runtimes = config.get_runtime_storage().await?;

    // Build live layers from composed bind mounts, if any.
    let live_layers = if !composed.contents.is_empty() {
        use spfs::runtime::{LiveLayer, LiveLayerContents};

        let mut layers = Vec::new();
        let mut contents = Vec::new();

        // For now, resolve all binds relative to the first source file's
        // directory; this matches the most common use case where contents
        // are defined in the working project spec.
        let spec_dir = composed
            .source_files
            .first()
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                crate::Error::ValidationFailed(
                    "No source files available to resolve bind mounts".to_string(),
                )
            })?;

        for bind in &composed.contents {
            let ll_bind = bind.to_live_layer_bind(spec_dir)?;
            contents.push(LiveLayerContents::BindMount(ll_bind));
        }

        if !contents.is_empty() {
            layers.push(LiveLayer {
                api: spfs::runtime::SpecApiVersion::V0Layer,
                contents,
            });
        }

        layers
    } else {
        Vec::new()
    };

    // Create runtime
    let mut runtime = match &options.name {
        Some(name) => {
            runtimes
                .create_named_runtime(name, options.keep, live_layers)
                .await?
        }
        None => runtimes.create_runtime(options.keep, live_layers).await?,
    };

    // Configure runtime
    runtime.config.mount_backend = config.filesystem.backend;
    runtime.config.secondary_repositories = config.get_secondary_runtime_repositories();
    runtime.status.editable = options.editable;

    // Resolve and push layer digests
    for layer_ref in &composed.layers {
        let digest = resolve_layer_reference(layer_ref, &repo).await?;
        runtime.push_digest(digest);
    }

    // If no layers, make editable by default
    if composed.layers.is_empty() {
        runtime.status.editable = true;
    }

    // If SPK integration is enabled, resolve packages and apply them
    // to the runtime before generating startup scripts.
    #[cfg(feature = "spk")]
    if !composed.packages.is_empty() {
        // Resolve repositories according to CLI/env flags
        let repo_list =
            crate::repository::resolve_spk_repositories(&options.repo_selection).await?;

        // Extract just the handles for the solver
        let repos: Vec<std::sync::Arc<spk_storage::RepositoryHandle>> =
            repo_list.into_iter().map(|(_, handle)| handle).collect();

        let pkg_opts = composed
            .package_options
            .as_ref()
            .cloned()
            .unwrap_or_default();

        let solution =
            crate::package::resolve_packages(&composed.packages, &pkg_opts, &repos).await?;

        crate::package::apply_solution_to_runtime(&mut runtime, &solution).await?;
    }

    // Generate environment startup script layer if needed
    if !composed.environment.is_empty() {
        let script = generate_startup_script(&composed.environment);
        let priority = get_priority(&composed.environment);

        // Create a temporary directory to hold the startup script
        let tmp_dir = TempDir::new()?;

        // Map STARTUP_FILES_LOCATION ("/spfs/etc/spfs/startup.d") into
        // a relative path rooted at the temp directory.
        let startup_root = STARTUP_FILES_LOCATION
            .strip_prefix(spfs::env::SPFS_DIR_PREFIX)
            .unwrap_or(STARTUP_FILES_LOCATION)
            .trim_start_matches('/');

        let script_name = format!("{:02}_spenv.sh", priority);
        let script_path = tmp_dir.path().join(startup_root).join(&script_name);

        if let Some(parent) = script_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&script_path, script)?;

        // Compute manifest and create a layer in the same repository
        let manifest = spfs::tracking::compute_manifest(tmp_dir.path()).await?;
        let layer = repo.create_layer_from_manifest(&manifest).await?;

        // Push script layer onto the stack
        runtime.push_digest(layer.digest()?);
        // tmp_dir dropped here after layer creation
    }

    // Save to storage (spfs-enter will read this)
    runtime.save_state_to_storage().await?;

    Ok(runtime)
}

/// Resolve layer reference to digest.
///
/// Supports:
/// - Tag references (e.g., "platform/centos7")
/// - Full digests (e.g., "A7USTIBXPXHMD5CYEIIOBMFLM3X77ESV...")
pub async fn resolve_layer_reference(
    reference: &str,
    repo: &RepositoryHandle,
) -> crate::Result<spfs::encoding::Digest> {
    // Try parsing as digest first
    if let Ok(digest) = reference.parse::<spfs::encoding::Digest>() {
        return Ok(digest);
    }

    // Parse reference as tag spec
    let tag_spec: TagSpec = reference.parse().map_err(|_| crate::Error::UnknownLayer {
        reference: reference.to_string(),
        similar: Vec::new(),
    })?;

    // Try resolving as tag
    match repo.resolve_tag(&tag_spec).await {
        Ok(tag) => Ok(tag.target),
        Err(_) => {
            // No suggestions for now - can be enhanced later
            Err(crate::Error::UnknownLayer {
                reference: reference.to_string(),
                similar: Vec::new(),
            })
        }
    }
}
