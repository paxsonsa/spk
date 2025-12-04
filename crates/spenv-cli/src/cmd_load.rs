// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the `spenv load` command.

use clap::Args;
use colored::Colorize;
use miette::Result;
use std::path::PathBuf;

/// Enter environment from current directory
#[derive(Debug, Args)]
pub struct CmdLoad {
    /// Start discovery from PATH
    #[clap(short = 'f', long, default_value = ".")]
    pub file: PathBuf,

    /// Enable in-tree discovery
    #[clap(long)]
    pub inherit: bool,

    /// Disable in-tree discovery
    #[clap(short = 'n', long)]
    pub no_inherit: bool,

    /// Additional .spenv.yaml to include
    #[clap(short = 'i', long = "include")]
    pub includes: Vec<String>,

    /// Repository selection flags
    #[clap(flatten)]
    pub repos: crate::RepoFlags,

    /// Make environment editable
    #[clap(short, long)]
    pub edit: bool,

    /// Keep runtime after exit
    #[clap(short, long)]
    pub keep: bool,

    /// Name the runtime
    #[clap(long)]
    pub name: Option<String>,

    /// Show what would be loaded without entering
    #[clap(long)]
    pub dry_run: bool,

    /// Command to run (default: $SHELL)
    #[clap(last = true)]
    pub command: Vec<String>,
}

impl CmdLoad {
    pub async fn run(&mut self) -> Result<i32> {
        // Get SPFS config
        let config = spfs::get_config().map_err(|e| miette::miette!("Failed to get config: {}", e))?;

        // Parse environment variables
        let env_includes = std::env::var("SPENV_INCLUDE")
            .ok()
            .map(|s| s.split(':').map(String::from).collect())
            .unwrap_or_default();

        let env_inherit = std::env::var("SPENV_INHERIT")
            .ok()
            .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"));

        let env_no_inherit = std::env::var("SPENV_NO_INHERIT")
            .ok()
            .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"));

        // Build discovery options
        let options = spenv::DiscoveryOptions {
            no_inherit: self.no_inherit || env_no_inherit,
            force_inherit: self.inherit || env_inherit,
            cli_includes: self.includes.clone(),
            env_includes,
        };

        // Discover specs
        let specs = spenv::discover_specs(&self.file, &options)?;

        if specs.is_empty() {
            return Err(miette::miette!(
                "No .spenv.yaml files discovered. Run 'spenv init' to create one."
            ));
        }

        // Compose environment
        let composed = spenv::compose_specs(&specs);

        // Dry run: just show what would be loaded
        if self.dry_run {
            println!("{}", "Discovered files:".bold());
            for spec in &specs {
                if let Some(path) = &spec.source_path {
                    println!("  - {}", path.display());
                }
            }
            println!();
            println!("{} layers:", composed.layers.len());
            for layer in &composed.layers {
                println!("  - {}", layer.green());
            }
            return Ok(0);
        }

        // Build repository selection from CLI flags
        let repo_selection = spenv::RepoSelection {
            enable_repo: self.repos.enable_repo.clone(),
            disable_repo: self.repos.disable_repo.clone(),
            no_local_repo: self.repos.no_local_repo,
            local_repo_only: self.repos.local_repo_only,
        };

        // Create runtime
        let runtime_options = spenv::RuntimeOptions {
            name: self.name.clone(),
            keep: self.keep,
            editable: self.edit,
            repo_selection,
        };

        tracing::info!("Creating runtime...");
        let runtime = spenv::create_runtime(&composed, &config, &runtime_options).await?;

        // Determine command to run
        let (command, args) = if self.command.is_empty() {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            (shell, Vec::<String>::new())
        } else {
            let mut iter = self.command.iter();
            let cmd = iter.next().unwrap().clone();
            let args: Vec<String> = iter.cloned().collect();
            (cmd, args)
        };

        // Build and exec into spfs-enter
        let cmd = spfs::build_command_for_runtime(&runtime, &command, args.iter().map(|s| s.as_str()))
            .map_err(|e| miette::miette!("Failed to build command: {}", e))?;

        tracing::info!("Entering runtime: {}", runtime.name());

        cmd.exec()
            .map(|_| 0)
            .map_err(|e| miette::miette!("Failed to execute runtime command: {}", e))
    }
}
