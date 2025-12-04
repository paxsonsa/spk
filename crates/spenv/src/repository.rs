// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

//! Repository selection helpers for spenv.

#[cfg(feature = "spk")]
use std::collections::HashSet;

#[cfg(feature = "spk")]
use std::sync::Arc;

#[cfg(feature = "spk")]
use crate::{Error, Result};

#[cfg(test)]
#[path = "./repository_test.rs"]
mod repository_test;

#[cfg(feature = "spk")]
pub type SpkRepositoryHandle = spk_storage::RepositoryHandle;

/// Parsed repository selection flags.
#[derive(Debug, Clone, Default)]
pub struct RepoSelection {
    pub enable_repo: Vec<String>,
    pub disable_repo: Vec<String>,
    pub no_local_repo: bool,
    pub local_repo_only: bool,
}

/// Resolve repositories according to selection flags, matching spk semantics.
#[cfg(feature = "spk")]
pub async fn resolve_spk_repositories(
    selection: &RepoSelection,
) -> Result<Vec<(String, Arc<SpkRepositoryHandle>)>> {
    let mut repos: Vec<(String, Arc<SpkRepositoryHandle>)> = Vec::new();
    let disabled: HashSet<&str> = selection.disable_repo.iter().map(String::as_str).collect();

    if !selection.no_local_repo && !disabled.contains("local") {
        if let Ok(local) = spk_storage::local_repository().await {
            repos.push(("local".into(), Arc::new(local.into())));
        }
    }

    if selection.local_repo_only {
        return Ok(repos);
    }

    let mut enabled = selection.enable_repo.clone();
    enabled.push("origin".into());

    for name in enabled {
        if disabled.contains(name.as_str()) {
            continue;
        }

        if let Some(pos) = repos.iter().position(|(n, _)| n == &name) {
            repos.remove(pos);
        }

        let repo = match name.as_str() {
            "local" => spk_storage::local_repository().await,
            _ => spk_storage::remote_repository(&name).await,
        };

        match repo {
            Ok(handle) => {
                repos.push((name, Arc::new(handle.into())));
            }
            Err(spk_storage::Error::SPFS(spfs::Error::UnknownRemoteName(_))) if name == "origin" => {
                // Default origin missing is allowed
                continue;
            }
            Err(err) => return Err(Error::ValidationFailed(format!(
                "Failed to open repository {name}: {err}"
            ))),
        }
    }

    Ok(repos)
}
