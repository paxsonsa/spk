// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! spenv - Cascading SPFS Environment Manager CLI

use clap::{Parser, Subcommand};
use miette::Result;

mod cmd_check;
mod cmd_init;
mod cmd_load;
mod cmd_lock;
mod cmd_shell;
mod cmd_show;

use cmd_check::CmdCheck;
use cmd_init::CmdInit;
use cmd_load::CmdLoad;
use cmd_lock::CmdLock;
use cmd_shell::CmdShell;
use cmd_show::CmdShow;

#[derive(Parser)]
#[clap(
    name = "spenv",
    about = "Cascading SPFS Environment Manager",
    version,
    long_about = "Manage SPFS environments through directory-based configuration files"
)]
struct Opt {
    #[clap(flatten)]
    logging: Logging,

    #[clap(subcommand)]
    cmd: Command,
}

#[derive(Parser)]
struct Logging {
    /// Increase verbosity (-v, -vv, -vvv)
    #[clap(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress non-error output
    #[clap(short, long)]
    quiet: bool,
}

#[derive(Parser, Clone, Debug, Default)]
pub struct RepoFlags {
    /// Enable additional repositories (name[@time])
    #[clap(long = "enable-repo", short = 'r', env = "SPENV_ENABLE_REPO")]
    pub enable_repo: Vec<String>,

    /// Disable repositories (name)
    #[clap(long = "disable-repo", env = "SPENV_DISABLE_REPO")]
    pub disable_repo: Vec<String>,

    /// Disable local repository
    #[clap(long = "no-local-repo", env = "SPENV_NO_LOCAL_REPO")]
    pub no_local_repo: bool,

    /// Use only the local repository
    #[clap(long = "local-repo-only", env = "SPENV_LOCAL_REPO_ONLY")]
    pub local_repo_only: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new .spenv.yaml file
    Init(CmdInit),

    /// Display resolved environment configuration
    Show(CmdShow),

    /// Enter environment from current directory
    Load(CmdLoad),

    /// Enter interactive shell in environment
    Shell(CmdShell),

    /// Generate or update lock file
    Lock(CmdLock),

    /// Verify environment matches lock file
    Check(CmdCheck),
}

impl Opt {
    async fn run(self) -> Result<i32> {
        // Setup logging
        let log_level = match (self.logging.quiet, self.logging.verbose) {
            (true, _) => tracing::Level::ERROR,
            (false, 0) => tracing::Level::WARN,
            (false, 1) => tracing::Level::INFO,
            (false, 2) => tracing::Level::DEBUG,
            (false, _) => tracing::Level::TRACE,
        };

        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .init();

        // Dispatch to command
        match self.cmd {
            Command::Init(mut cmd) => cmd.run().await,
            Command::Show(mut cmd) => cmd.run().await,
            Command::Load(mut cmd) => cmd.run().await,
            Command::Shell(mut cmd) => cmd.run().await,
            Command::Lock(mut cmd) => cmd.run().await,
            Command::Check(mut cmd) => cmd.run().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::parse();
    let code = opt.run().await?;
    std::process::exit(code);
}
