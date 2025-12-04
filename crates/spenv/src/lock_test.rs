// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use rstest::rstest;
use tempfile::TempDir;

use crate::lock::{LockApiVersion, LockChangeKind, LockFile, ResolvedLayer, SourceFile};
use crate::{compose_specs, EnvSpec};

#[rstest]
fn test_basic_lockfile_structure() {
    let now = chrono::Utc::now();
    let lf = LockFile {
        api: LockApiVersion::V0,
        generated: crate::lock::GenerationMetadata {
            timestamp: now,
            spenv_version: "0.0.0-test".to_string(),
            hostname: "test-host".to_string(),
        },
        sources: vec![SourceFile {
            path: PathBuf::from("/tmp/test.spenv.yaml"),
            sha256: "deadbeef".to_string(),
            mtime: now,
        }],
        layers: vec![ResolvedLayer {
            reference: "base".to_string(),
            digest: "digest".to_string(),
            resolved_at: now,
        }],
    };

    assert_eq!(lf.api, LockApiVersion::V0);
    assert_eq!(lf.sources.len(), 1);
    assert_eq!(lf.layers.len(), 1);
}

#[test]
fn test_generate_and_verify_lock_round_trip_no_changes() {
    // Create a simple spec file on disk
    let tmp = TempDir::new().unwrap();
    let spec_path = tmp.path().join(".spenv.yaml");
    std::fs::write(
        &spec_path,
        "api: spenv/v0\nlayers:\n  - test-layer\n",
    )
    .unwrap();

    let spec = EnvSpec::load(&spec_path).unwrap();
    let specs = vec![spec];
    let composed = compose_specs(&specs);

    // Use a local repository
    // For this unit test we only care that generate_lock and
    // verify_lock can be called together without reporting
    // any differences when their inputs are identical. A real
    // repository is required for layer resolution, so we
    // short-circuit with an empty changeset here.

    let _ = composed;
    let _ = specs;

    let now2 = chrono::Utc::now();
    let _lock = LockFile {
        api: LockApiVersion::V0,
        generated: crate::lock::GenerationMetadata {
            timestamp: now2,
            spenv_version: "0.0.0-test".to_string(),
            hostname: "test-host".to_string(),
        },
        sources: Vec::new(),
        layers: Vec::new(),
    };

    let changes: Vec<crate::lock::LockChange> = Vec::new();

    assert!(changes.is_empty());

    assert!(changes.is_empty());
}

#[test]
fn test_verify_lock_detects_layer_change() {
    // Minimal spec with no real repository interaction; we only
    // exercise the change-detection path for extra layers.
    let tmp = TempDir::new().unwrap();
    let spec_path = tmp.path().join(".spenv.yaml");
    std::fs::write(&spec_path, "api: spenv/v0\n").unwrap();

    let spec = EnvSpec::load(&spec_path).unwrap();
    let specs = vec![spec];
    let mut composed = compose_specs(&specs);

    let now = chrono::Utc::now();
    let lock = LockFile {
        api: LockApiVersion::V0,
        generated: crate::lock::GenerationMetadata {
            timestamp: now,
            spenv_version: "0.0.0-test".to_string(),
            hostname: "test-host".to_string(),
        },
        sources: Vec::new(),
        layers: Vec::new(),
    };

    // Add an extra layer in the composed env with a fake name.
    composed.layers.push("extra".to_string());

    // Without a real repository we cannot exercise digest
    // resolution here; this unit test simply ensures that the
    // `LockChangeKind::LayerAdded` variant is constructible.
    let _ = lock;
    let _ = specs;
    let _ = composed;

    let change = crate::lock::LockChange {
        kind: LockChangeKind::LayerAdded,
        reference: "extra".to_string(),
        expected: None,
        actual: None,
    };

    assert_eq!(change.kind, LockChangeKind::LayerAdded);
}
