// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn test_repo_selection_default() {
    let selection = RepoSelection::default();
    assert!(selection.enable_repo.is_empty());
    assert!(selection.disable_repo.is_empty());
    assert!(!selection.no_local_repo);
    assert!(!selection.local_repo_only);
}

#[test]
fn test_repo_selection_construction() {
    let selection = RepoSelection {
        enable_repo: vec!["staging".to_string()],
        disable_repo: vec!["origin".to_string()],
        no_local_repo: false,
        local_repo_only: false,
    };

    assert_eq!(selection.enable_repo, vec!["staging"]);
    assert_eq!(selection.disable_repo, vec!["origin"]);
}

// Note: Full integration tests for resolve_spk_repositories() require a
// configured SPK environment. The logic is tested indirectly through
// runtime creation integration tests.
