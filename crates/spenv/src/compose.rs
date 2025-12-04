// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Composition logic for merging multiple specs into a single environment.

use std::path::PathBuf;

use crate::bind::BindMount;
use crate::environment::EnvOp;
use crate::EnvSpec;

#[cfg(test)]
#[path = "./compose_test.rs"]
mod compose_test;

/// Composed environment from multiple specs.
#[derive(Debug, Clone, Default)]
pub struct ComposedEnvironment {
    /// Merged layer references (in order).
    pub layers: Vec<String>,

    /// Merged environment variable operations (in order).
    pub environment: Vec<EnvOp>,

    /// Merged bind mount specifications (in order).
    pub contents: Vec<BindMount>,

    /// Aggregated SPK package requests.
    pub packages: Vec<String>,

    /// Aggregated package options (last spec wins if set).
    pub package_options: Option<crate::spec::PackageOptions>,

    /// Source files that contributed to this composition.
    pub source_files: Vec<PathBuf>,
}

impl ComposedEnvironment {
    /// Create a new empty composed environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the environment has any layers.
    pub fn has_layers(&self) -> bool {
        !self.layers.is_empty()
    }

    /// Get the number of source files.
    pub fn source_count(&self) -> usize {
        self.source_files.len()
    }
}

/// Compose multiple specs into a single environment.
///
/// Specs are processed in order, with later specs layering on top of earlier ones.
pub fn compose_specs(specs: &[EnvSpec]) -> ComposedEnvironment {
    let mut composed = ComposedEnvironment::default();

    for spec in specs {
        // Layers: append in order (later specs layer on top)
        composed.layers.extend(spec.layers.iter().cloned());

        // Environment operations: append in order as well
        composed
            .environment
            .extend(spec.environment.iter().cloned());

        // Bind mounts: append in order
        composed
            .contents
            .extend(spec.contents.iter().cloned());

        // Packages: append in order
        composed.packages.extend(spec.packages.iter().cloned());

        // Package options: use the last non-None encountered
        if spec.package_options.is_some() {
            composed.package_options = spec.package_options.clone();
        }

        // Track source file
        if let Some(path) = &spec.source_path {
            composed.source_files.push(path.clone());
        }
    }

    composed
}
