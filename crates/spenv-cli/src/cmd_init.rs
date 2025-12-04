// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Implementation of the `spenv init` command.

use clap::Args;
use miette::Result;
use std::path::PathBuf;

/// Create a new .spenv.yaml file
#[derive(Debug, Args)]
pub struct CmdInit {
    /// Directory to create file in
    #[clap(default_value = ".")]
    path: PathBuf,

    /// Enable in-tree inheritance
    #[clap(long)]
    inherit: bool,

    /// Add initial layer reference
    #[clap(long = "layer")]
    layers: Vec<String>,

    /// Template to use: minimal, standard, full
    #[clap(long, default_value = "standard")]
    template: String,
}

impl CmdInit {
    pub async fn run(&mut self) -> Result<i32> {
        let spec_path = self.path.join(spenv::SPENV_FILENAME);

        // Check if file already exists
        if spec_path.exists() {
            return Err(miette::miette!(
                ".spenv.yaml already exists at {:?}",
                spec_path
            ));
        }

        // Generate template based on option
        let content = match self.template.as_str() {
            "minimal" => self.generate_minimal_template(),
            "full" => self.generate_full_template(),
            _ => self.generate_standard_template(),
        };

        // Write file
        std::fs::write(&spec_path, content)
            .map_err(|e| miette::miette!("Failed to write .spenv.yaml: {}", e))?;

        println!("Created .spenv.yaml at {:?}", spec_path);
        println!();
        println!("Next steps:");
        println!("  1. Edit the file to add your layers");
        println!("  2. Run 'spenv show' to preview the environment");
        println!("  3. Run 'spenv load' to enter the environment");

        Ok(0)
    }

    fn generate_minimal_template(&self) -> String {
        format!(
            "api: spenv/v0\n\
            inherit: {}\n\
            \n\
            layers: []\n",
            self.inherit
        )
    }

    fn generate_standard_template(&self) -> String {
        let layers_section = if self.layers.is_empty() {
            "# layers:\n\
            #   - platform/centos7\n\
            #   - dev-tools/latest\n"
                .to_string()
        } else {
            format!(
                "layers:\n{}\n",
                self.layers
                    .iter()
                    .map(|l| format!("  - {}", l))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };

        format!(
            "# spenv environment specification\n\
            # See: https://spkenv.dev/docs/spenv\n\
            \n\
            api: spenv/v0\n\
            \n\
            # Optional: Human-readable description\n\
            # description: \"My project environment\"\n\
            \n\
            # In-tree inheritance (default: false for security)\n\
            # When true, walks up directory tree loading parent .spenv.yaml files\n\
            # When false, only loads this file (recommended)\n\
            inherit: {}\n\
            \n\
            # Explicit includes (recommended over inherit: true)\n\
            # includes:\n\
            #   - ~/.config/spenv/defaults.spenv.yaml\n\
            #   - /team/shared/base.spenv.yaml\n\
            #   - ../shared/common.spenv.yaml\n\
            \n\
            # SPFS layers to load (tags, digests, or .spfs.yaml paths)\n\
            {}\
            \n\
            # Environment variable operations\n\
            # environment:\n\
            #   - prepend: PATH\n\
            #     value: /spfs/bin\n\
            #   - set: PROJECT_ROOT\n\
            #     value: /spfs/project\n\
            \n\
            # Bind mounts (host paths into /spfs)\n\
            # contents:\n\
            #   - bind: ./src\n\
            #     dest: /spfs/project/src\n",
            self.inherit, layers_section,
        )
    }

    fn generate_full_template(&self) -> String {
        format!(
            "# spenv environment specification\n\
            # Full example with all fields documented\n\
            \n\
            api: spenv/v0\n\
            \n\
            description: \"Full example environment\"\n\
            \n\
            inherit: {}\n\
            \n\
            includes:\n\
            #   - ~/.config/spenv/defaults.spenv.yaml\n\
            #   - /team/shared/base.spenv.yaml\n\
            \n\
            layers:\n\
            #   - platform/centos7\n\
            #   - dev-tools/latest\n\
            \n\
            packages:\n\
            #   - python/3.9\n\
            #   - cmake/3.20\n\
            \n\
            package_options:\n\
            #   binary_only: true\n\
            #   solver: step\n\
            #\n\
            # Note: Repository selection is controlled via CLI flags, not in spec:\n\
            #   spenv load --enable-repo origin --disable-repo local\n\
            #   See docs for SPENV_ENABLE_REPO and other environment variables\n\
            \n\
            environment:\n\
            #   - set: PROJECT_ROOT\n\
            #     value: /spfs/project\n\
            #   - prepend: PATH\n\
            #     value: /spfs/bin\n\
            #   - priority: 50\n\
            \n\
            contents:\n\
            #   - bind: ./src\n\
            #     dest: /spfs/project/src\n\
            #     readonly: false\n\
            \n\
            lock:\n\
            #   enabled: true\n\
            #   strict: false\n",
            self.inherit
        )
    }
}
