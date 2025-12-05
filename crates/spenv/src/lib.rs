// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! spenv - Cascading SPFS Environment Manager
//!
//! This crate provides the core library for managing cascading SPFS environments
//! through directory-based configuration files (`.spenv.yaml`).
//!
//! # Overview
//!
//! spenv enables users to define layered software environments that compose based
//! on explicit includes and optional directory hierarchy inheritance, with a
//! secure-by-default design where `inherit: false` prevents untrusted parent
//! directories from modifying environments.
//!
//! # Example
//!
//! ```yaml
//! # .spenv.yaml
//! api: spenv/v0
//! description: "My project environment"
//!
//! # Security: don't walk up directory tree (default)
//! inherit: false
//!
//! # Explicit includes (recommended)
//! includes:
//!   - ~/.config/spenv/defaults.spenv.yaml
//!   - /team/shared/base.spenv.yaml
//!
//! # SPFS layers to load
//! layers:
//!   - platform/centos7
//!   - dev-tools/latest
//! ```

pub mod bind;
pub mod compose;
pub mod discovery;
pub mod environment;
pub mod error;
pub mod lock;
#[cfg(feature = "spk")]
pub mod package;
pub mod repository;
pub mod runtime;
pub mod spec;

pub use bind::BindMount;
pub use compose::{ComposedEnvironment, compose_specs};
pub use discovery::{DiscoveryOptions, discover_specs};
pub use environment::{EnvOp, generate_startup_script};
pub use error::{Error, Result};
pub use lock::{LockChange, LockChangeKind, LockFile, generate_lock, verify_lock};
pub use repository::RepoSelection;
pub use runtime::{RuntimeOptions, create_runtime};
pub use spec::{ApiVersion, EnvSpec, PackageOptions};

/// Well-known filename for environment specs.
pub const SPENV_FILENAME: &str = ".spenv.yaml";

/// Well-known filename for local overrides.
pub const SPENV_LOCAL_FILENAME: &str = ".spenv.local.yaml";

/// Well-known filename for lock files.
pub const SPENV_LOCK_FILENAME: &str = ".spenv.lock.yaml";
