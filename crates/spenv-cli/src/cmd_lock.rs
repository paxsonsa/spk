// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Generate or update lock files for spenv environments.

use std::path::PathBuf;

use clap::Args;
use miette::Result;

/// Generate or update lock file
#[derive(Debug, Args)]
pub struct CmdLock {
    /// Start discovery from PATH
    #[clap(short, long, default_value = ".")]
    file: PathBuf,

    /// Update existing lock file
    #[clap(long)]
    update: bool,

    /// Force regeneration even if up-to-date
    #[clap(long)]
    force: bool,

    /// Verify lock is current (exit 1 if not)
    #[clap(long)]
    check: bool,

    /// Repository selection flags
    #[clap(flatten)]
    repos: crate::RepoFlags,
}

impl CmdLock {
    pub async fn run(&mut self) -> Result<i32> {
        let config =
            spfs::get_config().map_err(|e| miette::miette!("Failed to get config: {e}"))?;

        // Discover specs using default discovery options from the given path.
        let options = spenv::DiscoveryOptions::default();
        let specs = spenv::discover_specs(&self.file, &options)?;
        let composed = spenv::compose_specs(&specs);

        let repo = config
            .get_local_repository_handle()
            .await
            .map_err(|e| miette::miette!("Failed to open local repository: {e}"))?;

        // Determine lock file path (adjacent to starting path).
        let lock_path = self.file.join(spenv::SPENV_LOCK_FILENAME);

        if self.check {
            // Verify mode
            if !lock_path.exists() {
                eprintln!("No lock file found at {:?}", lock_path);
                return Ok(2);
            }

            let lock_yaml = std::fs::read_to_string(&lock_path)
                .map_err(|e| miette::miette!("Failed to read lock file {:?}: {e}", lock_path))?;
            let lock: spenv::LockFile = serde_yaml::from_str(&lock_yaml)
                .map_err(|e| miette::miette!("Failed to parse lock file {:?}: {e}", lock_path))?;

            let changes = spenv::verify_lock(&lock, &specs, &composed, &repo).await?;

            if !changes.is_empty() {
                eprintln!("Lock file is out of date:");
                for change in &changes {
                    eprintln!("  - {:?}: {}", change.kind, change.reference);
                }
                return Ok(1);
            }

            println!("Lock file is up to date");
            return Ok(0);
        }

        // Generate / update mode
        if lock_path.exists() && !self.update && !self.force {
            return Err(miette::miette!(
                "Lock file already exists at {:?}. Use --update or --force",
                lock_path
            ));
        }

        let lock = spenv::generate_lock(&specs, &composed, &repo).await?;
        let lock_yaml = serde_yaml::to_string(&lock)
            .map_err(|e| miette::miette!("Failed to serialize lock file {:?}: {e}", lock_path))?;

        std::fs::write(&lock_path, lock_yaml)
            .map_err(|e| miette::miette!("Failed to write lock file {:?}: {e}", lock_path))?;
        println!("Generated lock file: {:?}", lock_path);

        Ok(0)
    }
}
