// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Discovery algorithm for finding and loading .spenv.yaml files.

use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(test)]
#[path = "./discovery_test.rs"]
mod discovery_test;

use crate::{EnvSpec, SPENV_FILENAME, SPENV_LOCAL_FILENAME};

/// Global cache to prevent circular includes.
static SEEN_SPEC_FILES: Lazy<Mutex<HashSet<PathBuf>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

/// Clear the circular include cache (used in tests).
pub fn clear_seen_spec_cache() {
    let mut seen = SEEN_SPEC_FILES.lock().unwrap();
    seen.clear();
}

/// Options for discovery behavior.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryOptions {
    /// Disable in-tree inheritance (from --no-inherit or SPENV_NO_INHERIT).
    pub no_inherit: bool,

    /// Enable in-tree inheritance (from --inherit or SPENV_INHERIT).
    pub force_inherit: bool,

    /// Additional includes from CLI (from --include flags).
    pub cli_includes: Vec<String>,

    /// Additional includes from environment (from SPENV_INCLUDE).
    pub env_includes: Vec<String>,
}

/// Discover all applicable .spenv.yaml files.
///
/// Returns specs in composition order (earlier specs are layered first).
pub fn discover_specs<P: AsRef<Path>>(
    start_path: P,
    options: &DiscoveryOptions,
) -> crate::Result<Vec<EnvSpec>> {
    clear_seen_spec_cache();

    let mut specs = Vec::new();

    // Step 1: Process CLI includes (highest priority, go first in composition)
    for include_path in &options.cli_includes {
        let spec = load_spec_from_include(include_path, None)?;
        specs.push(spec);
    }

    // Step 2: Process environment variable includes
    for include_path in &options.env_includes {
        let spec = load_spec_from_include(include_path, None)?;
        specs.push(spec);
    }

    // Step 3: Discover in-tree specs
    let in_tree_specs = discover_in_tree(start_path.as_ref(), options)?;
    specs.extend(in_tree_specs);

    // Step 4: Resolve all includes recursively
    let mut all_specs = resolve_all_includes(specs)?;

    // Step 5: Load local override if it exists
    let start = resolve_start_path(start_path.as_ref());
    let local_path = start.join(SPENV_LOCAL_FILENAME);
    if local_path.is_file() {
        let local_spec = EnvSpec::load(&local_path)?;
        all_specs.push(local_spec);
    }

    Ok(all_specs)
}

/// Resolve starting path, preferring $PWD to preserve symlinks.
fn resolve_start_path(start_path: &Path) -> PathBuf {
    if start_path.is_absolute() {
        start_path.to_owned()
    } else {
        match std::env::var("PWD").ok() {
            Some(pwd) => PathBuf::from(pwd).join(start_path),
            None => std::env::current_dir()
                .unwrap_or_default()
                .join(start_path),
        }
    }
}

/// Discover specs in directory tree (walking up parents).
fn discover_in_tree(start_path: &Path, options: &DiscoveryOptions) -> crate::Result<Vec<EnvSpec>> {
    let start = resolve_start_path(start_path);
    let mut specs = Vec::new();
    let mut current = start.clone();

    // Always try to load the starting point's spec
    let start_spec_path = current.join(SPENV_FILENAME);
    if start_spec_path.is_file() {
        let spec = EnvSpec::load(&start_spec_path)?;
        specs.push(spec.clone());

        // Check if we should walk up tree
        let should_inherit = if options.force_inherit {
            true // --inherit overrides spec
        } else if options.no_inherit {
            false // --no-inherit overrides spec
        } else {
            spec.inherit // Use spec's setting (default: false)
        };

        if !should_inherit {
            return Ok(specs); // Don't walk up
        }
    } else if options.no_inherit {
        // --no-inherit specified but no spec at start path
        return Err(crate::Error::NotFoundAtPath(current));
    } else {
        // No spec at start path and no --no-inherit, try to find one
        // by walking up (but only if not explicitly disabled)
    }

    // Walk up directory tree
    while current.pop() {
        let spec_path = current.join(SPENV_FILENAME);

        if spec_path.is_file() {
            let spec = EnvSpec::load(&spec_path)?;
            specs.insert(0, spec.clone()); // Parents go first

            // Check inherit flag to stop walking
            if !spec.inherit {
                break; // Stop walking up tree
            }
        }
    }

    // If we walked the whole tree and found nothing
    if specs.is_empty() {
        return Err(crate::Error::NotFoundInTree(start));
    }

    Ok(specs)
}

/// Load a spec from an include path (absolute, home-relative, or relative).
fn load_spec_from_include(include_path: &str, base_dir: Option<&Path>) -> crate::Result<EnvSpec> {
    let path = resolve_include_path(include_path, base_dir)?;

    // Check for circular includes
    {
        let mut seen = SEEN_SPEC_FILES.lock().unwrap();
        if seen.contains(&path) {
            return Err(crate::Error::CircularInclude(path));
        }
        seen.insert(path.clone());
    }

    EnvSpec::load(&path)
}

/// Resolve include path to absolute canonical path.
fn resolve_include_path(include: &str, base_dir: Option<&Path>) -> crate::Result<PathBuf> {
    let path = if include.starts_with('~') {
        // Home-relative
        let home = dirs::home_dir().ok_or_else(|| {
            crate::Error::ValidationFailed("Cannot resolve ~ without HOME".to_string())
        })?;
        let rel = include.strip_prefix("~/").unwrap_or(include);
        home.join(rel)
    } else if Path::new(include).is_absolute() {
        // Absolute
        PathBuf::from(include)
    } else {
        // Relative - need base_dir
        let base = base_dir.ok_or_else(|| {
            crate::Error::ValidationFailed(format!(
                "Cannot resolve relative include '{}' without base directory",
                include
            ))
        })?;
        base.join(include)
    };

    dunce::canonicalize(&path).map_err(|e| crate::Error::IncludeNotFound {
        path: path.clone(),
        error: e,
    })
}

/// Recursively resolve all includes in specs.
fn resolve_all_includes(specs: Vec<EnvSpec>) -> crate::Result<Vec<EnvSpec>> {
    let mut result = Vec::new();

    for spec in specs {
        // Process includes before this spec
        for include_path in &spec.includes {
            let base_dir = spec.source_path.as_ref().and_then(|p| p.parent());

            let include_spec = load_spec_from_include(include_path, base_dir)?;

            // Recursively resolve includes from this include
            let nested = resolve_all_includes(vec![include_spec])?;
            result.extend(nested);
        }

        result.push(spec);
    }

    Ok(result)
}
