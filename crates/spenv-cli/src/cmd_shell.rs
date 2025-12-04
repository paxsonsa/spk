// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the `spenv shell` command.

use clap::Args;
use miette::Result;
use std::path::PathBuf;

/// Enter interactive shell in environment
#[derive(Debug, Args)]
pub struct CmdShell {
    /// Start discovery from PATH
    #[clap(short = 'f', long, default_value = ".")]
    file: PathBuf,

    /// Enable in-tree discovery
    #[clap(long)]
    inherit: bool,

    /// Disable in-tree discovery
    #[clap(short = 'n', long)]
    no_inherit: bool,

    /// Additional .spenv.yaml to include
    #[clap(short = 'i', long = "include")]
    includes: Vec<String>,

    /// Repository selection flags
    #[clap(flatten)]
    repos: crate::RepoFlags,

    /// Make environment editable
    #[clap(short, long)]
    edit: bool,

    /// Keep runtime after exit
    #[clap(short, long)]
    keep: bool,

    /// Name the runtime
    #[clap(long)]
    name: Option<String>,

    /// Shell to use
    #[clap(long)]
    shell: Option<String>,
}

impl CmdShell {
    pub async fn run(&mut self) -> Result<i32> {
        // Set command to shell
        let shell = self
            .shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/bash".to_string());

        // Build load command with our settings
        let mut load_cmd = super::cmd_load::CmdLoad {
            file: self.file.clone(),
            inherit: self.inherit,
            no_inherit: self.no_inherit,
            includes: self.includes.clone(),
            repos: self.repos.clone(),
            edit: self.edit,
            keep: self.keep,
            name: self.name.clone(),
            dry_run: false,
            command: vec![shell],
        };

        load_cmd.run().await
    }
}
