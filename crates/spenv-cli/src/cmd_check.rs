// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Verify that the current environment matches the lock file.

use std::path::PathBuf;

use clap::Args;
use miette::Result;

/// Verify environment matches lock file
#[derive(Debug, Args)]
pub struct CmdCheck {
    /// Start discovery from PATH
    #[clap(short, long, default_value = ".")]
    file: PathBuf,

    /// Exit with error on mismatch
    #[clap(long)]
    strict: bool,

    /// Repository selection flags
    #[clap(flatten)]
    repos: crate::RepoFlags,
}

impl CmdCheck {
    pub async fn run(&mut self) -> Result<i32> {
        let config =
            spfs::get_config().map_err(|e| miette::miette!("Failed to get config: {e}"))?;

        // Discover specs and compose environment
        let options = spenv::DiscoveryOptions::default();
        let specs = spenv::discover_specs(&self.file, &options)?;
        let composed = spenv::compose_specs(&specs);

        let repo = config
            .get_local_repository_handle()
            .await
            .map_err(|e| miette::miette!("Failed to open local repository: {e}"))?;

        // Load lock file
        let lock_path = self.file.join(spenv::SPENV_LOCK_FILENAME);

        if !lock_path.exists() {
            if self.strict {
                return Err(miette::miette!("No lock file found at {:?}", lock_path));
            } else {
                println!("Warning: No lock file found");
                return Ok(2);
            }
        }

        let lock_yaml = std::fs::read_to_string(&lock_path)
            .map_err(|e| miette::miette!("Failed to read lock file {:?}: {e}", lock_path))?;
        let lock: spenv::LockFile = serde_yaml::from_str(&lock_yaml)
            .map_err(|e| miette::miette!("Failed to parse lock file {:?}: {e}", lock_path))?;

        // Verify
        let changes = spenv::verify_lock(&lock, &specs, &composed, &repo).await?;

        if changes.is_empty() {
            println!("✓ Environment matches lock file");
            return Ok(0);
        }

        // Report changes
        if self.strict {
            eprintln!("Error: Environment differs from lock file:");
        } else {
            println!("Warning: Environment differs from lock file:");
        }

        for change in &changes {
            match &change.kind {
                spenv::LockChangeKind::LayerDigestChanged => {
                    println!("  - Layer '{}' digest changed", change.reference);
                    if let (Some(exp), Some(act)) = (&change.expected, &change.actual) {
                        println!("    Expected: {}", exp);
                        println!("    Actual:   {}", act);
                    }
                }
                spenv::LockChangeKind::SourceFileChanged => {
                    println!("  - Source file '{}' was modified", change.reference);
                }
                _ => {
                    println!("  - {:?}: {}", change.kind, change.reference);
                }
            }
        }

        if self.strict {
            return Ok(1);
        }

        println!("\nRun 'spenv lock --update' to update the lock file");
        Ok(0)
    }
}
