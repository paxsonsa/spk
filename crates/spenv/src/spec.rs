// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Spec file parsing and data types for .spenv.yaml files.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::bind::BindMount;
use crate::environment::EnvOp;

#[cfg(test)]
#[path = "./spec_test.rs"]
mod spec_test;

/// API version for spec files.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum ApiVersion {
    #[serde(rename = "spenv/v0")]
    V0,
}

impl Default for ApiVersion {
    fn default() -> Self {
        Self::V0
    }
}

/// Helper for two-stage deserialization to determine API version first.
#[derive(Deserialize)]
struct ApiVersionMapping {
    #[serde(default)]
    api: ApiVersion,
}

/// Options controlling SPK package resolution.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PackageOptions {
    /// Resolve packages in binary-only mode (no source builds).
    #[serde(default = "default_binary_only")]
    pub binary_only: bool,

    /// Additional repositories to search (names from SPFS config).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<String>,

    /// Optional solver name: "step" (default) or "resolvo".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver: Option<String>,
}

fn default_binary_only() -> bool {
    true
}

/// Main environment specification from a .spenv.yaml file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnvSpec {
    /// API version identifier.
    pub api: ApiVersion,

    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// In-tree inheritance control.
    /// When false (default), stops walking up directory tree.
    /// When true, discovers .spenv.yaml files in parent directories.
    #[serde(default)] // Default is false for security
    pub inherit: bool,

    /// Out-of-tree includes loaded before in-tree discovery.
    /// Can use absolute paths, home-relative (~/) paths, or relative paths.
    /// Relative paths are resolved relative to this file's directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,

    /// SPFS layers to load (tags, digests, or paths to .spfs.yaml files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,

    /// Environment variable operations (set, prepend, append, comment, priority).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<EnvOp>,

    /// Bind mounts into the runtime (`contents:` field).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<BindMount>,

    /// SPK package requests (optional, requires `spk` feature).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<String>,

    /// Options controlling package resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_options: Option<PackageOptions>,

    /// Path to the file this was loaded from (not serialized).
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

impl EnvSpec {
    /// Parse spec from YAML string.
    pub fn from_yaml<S: Into<String>>(yaml: S) -> crate::Result<Self> {
        let yaml = yaml.into();

        // Stage 1: Parse to get API version
        let value: serde_yaml::Value =
            serde_yaml::from_str(&yaml).map_err(|e| crate::Error::InvalidYaml {
                error: e,
                yaml_content: yaml.clone(),
            })?;

        let with_version: ApiVersionMapping =
            serde_yaml::from_value(value.clone()).map_err(|e| crate::Error::InvalidYaml {
                error: e,
                yaml_content: yaml.clone(),
            })?;

        // Stage 2: Deserialize based on version
        match with_version.api {
            ApiVersion::V0 => {
                serde_yaml::from_value(value).map_err(|e| crate::Error::InvalidYaml {
                    error: e,
                    yaml_content: yaml,
                })
            }
        }
    }

    /// Load spec from file path.
    pub fn load<P: AsRef<std::path::Path>>(path: P) -> crate::Result<Self> {
        let path = path.as_ref();
        let yaml = std::fs::read_to_string(path).map_err(|e| crate::Error::ReadFailed {
            path: path.to_path_buf(),
            error: e,
        })?;

        let mut spec = Self::from_yaml(yaml)?;
        spec.source_path = Some(path.to_path_buf());
        Ok(spec)
    }

    /// Validate spec after loading.
    pub fn validate(&self) -> crate::Result<()> {
        // Validate that source_path is set
        if self.source_path.is_none() {
            return Err(crate::Error::ValidationFailed(
                "source_path must be set".to_string(),
            ));
        }

        Ok(())
    }

    /// Resolve relative includes to absolute paths.
    pub fn resolve_includes(&self) -> crate::Result<Vec<PathBuf>> {
        let base_dir = self
            .source_path
            .as_ref()
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                crate::Error::ValidationFailed(
                    "Cannot resolve includes without source_path".to_string(),
                )
            })?;

        let mut resolved = Vec::new();
        for include in &self.includes {
            let path = if include.starts_with('~') {
                // Home-relative path
                let home = dirs::home_dir().ok_or_else(|| {
                    crate::Error::ValidationFailed("Cannot resolve ~ without HOME".to_string())
                })?;
                let rel_path = include.strip_prefix("~/").unwrap_or(include);
                home.join(rel_path)
            } else if std::path::Path::new(include).is_absolute() {
                // Absolute path
                PathBuf::from(include)
            } else {
                // Relative path - resolve relative to this spec's directory
                base_dir.join(include)
            };

            let canonical =
                dunce::canonicalize(&path).map_err(|e| crate::Error::IncludeNotFound {
                    path: path.clone(),
                    error: e,
                })?;

            resolved.push(canonical);
        }

        Ok(resolved)
    }
}

impl Default for EnvSpec {
    fn default() -> Self {
        Self {
            api: ApiVersion::default(),
            description: None,
            inherit: false,
            includes: Vec::new(),
            layers: Vec::new(),
            environment: Vec::new(),
            contents: Vec::new(),
            packages: Vec::new(),
            package_options: None,
            source_path: None,
        }
    }
}
