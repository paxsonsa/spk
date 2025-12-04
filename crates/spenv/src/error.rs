// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Error types for spenv operations.

use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

/// Convenience Result type with spenv Error.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during spenv operations.
#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    /// No .spenv.yaml found in directory tree
    #[error("No .spenv.yaml found in {0:?} or any parent directory")]
    #[diagnostic(
        code(spenv::not_found_in_tree),
        help("Create a .spenv.yaml file with 'spenv init' or specify a path with -f")
    )]
    NotFoundInTree(PathBuf),

    /// .spenv.yaml not found at specified path
    #[error(".spenv.yaml not found at {0:?}")]
    #[diagnostic(code(spenv::not_found_at_path))]
    NotFoundAtPath(PathBuf),

    /// Invalid YAML in spec file
    #[error("Invalid .spenv.yaml file: {error}")]
    #[diagnostic(
        code(spenv::invalid_yaml),
        help("Check YAML syntax and ensure 'api: spenv/v0' is present")
    )]
    InvalidYaml {
        #[source]
        error: serde_yaml::Error,
        yaml_content: String,
    },

    /// Failed to read file
    #[error("Failed to read file: {path:?}")]
    #[diagnostic(code(spenv::read_failed))]
    ReadFailed {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },

    /// Include file not found
    #[error("Include file not found: {path:?}")]
    #[diagnostic(
        code(spenv::include_not_found),
        help("Check that the include path is correct and the file exists")
    )]
    IncludeNotFound {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },

    /// Circular include detected
    #[error("Circular include detected: {0:?}")]
    #[diagnostic(
        code(spenv::circular_include),
        help("Remove the circular reference in your includes")
    )]
    CircularInclude(PathBuf),

    /// Validation error
    #[error("Validation failed: {0}")]
    #[diagnostic(code(spenv::validation_failed))]
    ValidationFailed(String),

    /// Unknown layer reference
    #[error("Unknown layer reference: {reference}")]
    #[diagnostic(
        code(spenv::unknown_layer),
        help("{}", suggestion_message(similar))
    )]
    UnknownLayer {
        reference: String,
        similar: Vec<String>,
    },

    /// SPFS error passthrough
    #[error(transparent)]
    #[diagnostic(code(spenv::spfs_error))]
    Spfs(#[from] spfs::Error),

    /// IO error passthrough
    #[error(transparent)]
    #[diagnostic(code(spenv::io_error))]
    Io(#[from] std::io::Error),
}

fn suggestion_message(similar: &[String]) -> String {
    if similar.is_empty() {
        "Check that the layer reference is correct".to_string()
    } else {
        format!("Did you mean one of: {}?", similar.join(", "))
    }
}
