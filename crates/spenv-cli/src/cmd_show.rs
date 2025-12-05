// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the `spenv show` command.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use miette::Result;

/// Display resolved environment configuration
#[derive(Debug, Args)]
pub struct CmdShow {
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

    /// Show discovered files
    #[clap(long)]
    files: bool,

    /// Show layer stack
    #[clap(long)]
    layers: bool,

    /// Show all information
    #[clap(long)]
    all: bool,

    /// Output format: table, yaml, json
    #[clap(long, default_value = "table")]
    format: String,
}

impl CmdShow {
    pub async fn run(&mut self) -> Result<i32> {
        // Parse SPENV_INCLUDE environment variable
        let env_includes = std::env::var("SPENV_INCLUDE")
            .ok()
            .map(|s| s.split(':').map(String::from).collect())
            .unwrap_or_default();

        // Check SPENV_INHERIT / SPENV_NO_INHERIT
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

        // Compose environment
        let composed = spenv::compose_specs(&specs);

        // Display based on flags
        let show_files = self.files || self.all || (!self.layers && !self.files);
        let show_layers = self.layers || self.all || (!self.layers && !self.files);

        if self.format == "yaml" {
            self.show_yaml(&specs, &composed)?;
        } else if self.format == "json" {
            self.show_json(&specs, &composed)?;
        } else {
            // Table format
            if show_files {
                self.show_files_table(&specs)?;
            }
            if show_files && show_layers {
                println!();
            }
            if show_layers {
                self.show_layers_table(&composed)?;
            }
        }

        Ok(0)
    }

    fn show_files_table(&self, specs: &[spenv::EnvSpec]) -> Result<()> {
        println!("{}", "Discovered Files:".bold());
        println!();

        for (i, spec) in specs.iter().enumerate() {
            let path = spec
                .source_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string());

            let inherit_marker = if spec.inherit { " [inherit]" } else { "" };
            let includes_marker = if !spec.includes.is_empty() {
                format!(" [includes: {}]", spec.includes.len())
            } else {
                String::new()
            };

            println!(
                "  {}. {}{}{}",
                i + 1,
                path.cyan(),
                inherit_marker.yellow(),
                includes_marker.blue()
            );

            if let Some(desc) = &spec.description {
                println!("     {}", desc.dimmed());
            }
        }

        println!();
        println!("Total: {} file(s)", specs.len());

        Ok(())
    }

    fn show_layers_table(&self, composed: &spenv::ComposedEnvironment) -> Result<()> {
        println!("{}", "Merged Layer Stack:".bold());
        println!();

        if composed.layers.is_empty() {
            println!("  {}", "(no layers)".dimmed());
        } else {
            for (i, layer) in composed.layers.iter().enumerate() {
                println!("  {}. {}", i + 1, layer.green());
            }
        }

        println!();
        println!("Total: {} layer(s)", composed.layers.len());

        // Display environment operations if present
        if !composed.environment.is_empty() {
            println!();
            println!("{}", "Environment Variables:".bold());
            println!();

            for (i, op) in composed.environment.iter().enumerate() {
                match op {
                    spenv::EnvOp::Set(s) => {
                        println!("  {}. {} = {}", i + 1, s.set.cyan(), s.value.green());
                    }
                    spenv::EnvOp::Prepend(p) => {
                        println!(
                            "  {}. {} = {} + ${}",
                            i + 1,
                            p.prepend.cyan(),
                            p.value.green(),
                            p.prepend
                        );
                    }
                    spenv::EnvOp::Append(a) => {
                        println!(
                            "  {}. {} = ${} + {}",
                            i + 1,
                            a.append.cyan(),
                            a.append,
                            a.value.green()
                        );
                    }
                    spenv::EnvOp::Comment(c) => {
                        println!("  # {}", c.comment.dimmed());
                    }
                    spenv::EnvOp::Priority(p) => {
                        println!("  [priority: {}]", p.priority.to_string().yellow());
                    }
                }
            }
        }

        Ok(())
    }

    fn show_yaml(
        &self,
        specs: &[spenv::EnvSpec],
        composed: &spenv::ComposedEnvironment,
    ) -> Result<()> {
        println!("# Discovered Files:");
        for spec in specs {
            if let Some(path) = &spec.source_path {
                println!("# - {}", path.display());
            }
        }
        println!();

        println!("# Composed Environment:");
        println!("layers:");
        for layer in &composed.layers {
            println!("  - {}", layer);
        }

        if !composed.environment.is_empty() {
            println!();
            println!("environment:");
            for op in &composed.environment {
                match op {
                    spenv::EnvOp::Set(s) => {
                        println!("  - set: {}", s.set);
                        println!("    value: {}", s.value);
                    }
                    spenv::EnvOp::Prepend(p) => {
                        println!("  - prepend: {}", p.prepend);
                        println!("    value: {}", p.value);
                        if let Some(sep) = &p.separator {
                            println!("    separator: {}", sep);
                        }
                    }
                    spenv::EnvOp::Append(a) => {
                        println!("  - append: {}", a.append);
                        println!("    value: {}", a.value);
                        if let Some(sep) = &a.separator {
                            println!("    separator: {}", sep);
                        }
                    }
                    spenv::EnvOp::Comment(c) => {
                        println!("  - comment: {}", c.comment);
                    }
                    spenv::EnvOp::Priority(p) => {
                        println!("  - priority: {}", p.priority);
                    }
                }
            }
        }

        Ok(())
    }

    fn show_json(
        &self,
        specs: &[spenv::EnvSpec],
        composed: &spenv::ComposedEnvironment,
    ) -> Result<()> {
        let files: Vec<String> = specs
            .iter()
            .filter_map(|s| s.source_path.as_ref().map(|p| p.display().to_string()))
            .collect();

        // Simple manual JSON output to avoid serde_json dependency in CLI
        println!("{{");
        println!(
            "  \"discovered_files\": [{}],",
            files
                .iter()
                .map(|f| format!("\"{}\"", f))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "  \"layers\": [{}],",
            composed
                .layers
                .iter()
                .map(|l| format!("\"{}\"", l))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("  \"total_files\": {},", specs.len());
        println!("  \"total_layers\": {}", composed.layers.len());
        println!("}}");

        Ok(())
    }
}
