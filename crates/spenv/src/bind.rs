// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Bind mount specifications for `contents:` in .spenv.yaml.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::Error;

#[cfg(test)]
#[path = "./bind_test.rs"]
mod bind_test;

/// Bind mount specification from a `.spenv.yaml` `contents:` entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BindMount {
    /// Source path on the host (relative, absolute, or `~/`)
    pub bind: String,
    /// Destination path inside `/spfs`.
    pub dest: String,
    /// Whether the bind should be read-only (reserved for future use).
    #[serde(default)]
    pub readonly: bool,
}

impl BindMount {
    /// Convert this spec into the SPFS `BindMount` used in live layers.
    pub fn to_live_layer_bind(
        &self,
        spec_dir: &std::path::Path,
    ) -> crate::Result<spfs::runtime::BindMount> {
        // Resolve source path
        let src = if self.bind.starts_with('~') {
            let home = dirs::home_dir().ok_or_else(|| {
                Error::ValidationFailed("Cannot resolve ~ without HOME".to_string())
            })?;
            let rel = self.bind.strip_prefix("~/").unwrap_or(&self.bind);
            home.join(rel)
        } else if PathBuf::from(&self.bind).is_absolute() {
            PathBuf::from(&self.bind)
        } else {
            spec_dir.join(&self.bind)
        };

        // Canonicalize to ensure a real path on disk.
        let src = dunce::canonicalize(&src).map_err(|e| {
            Error::ValidationFailed(format!(
                "Bind mount source not found or invalid: {} ({e})",
                src.display()
            ))
        })?;

        Ok(spfs::runtime::BindMount {
            src,
            dest: self.dest.clone(),
        })
    }
}
