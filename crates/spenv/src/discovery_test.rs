// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use rstest::rstest;
use tempfile::TempDir;

use super::*;

fn create_spec_file(dir: &Path, content: &str) {
    let path = dir.join(SPENV_FILENAME);
    std::fs::write(path, content).expect("Failed to write spec file");
}

#[rstest]
fn test_discover_single_spec() {
    let tmp = TempDir::new().unwrap();
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
layers:
  - test-layer
"#,
    );

    let options = DiscoveryOptions::default();
    let specs = discover_specs(tmp.path(), &options).expect("Should discover spec");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].layers, vec!["test-layer"]);
}

#[rstest]
fn test_inherit_false_stops_discovery() {
    let tmp = TempDir::new().unwrap();
    let child = tmp.path().join("child");
    std::fs::create_dir(&child).unwrap();

    // Parent spec
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
inherit: false
layers:
  - parent-layer
"#,
    );

    // Child spec with inherit: false (default)
    create_spec_file(
        &child,
        r#"
api: spenv/v0
layers:
  - child-layer
"#,
    );

    let options = DiscoveryOptions::default();
    let specs = discover_specs(&child, &options).expect("Should discover spec");

    // Should only find child spec since inherit defaults to false
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].layers, vec!["child-layer"]);
}

#[rstest]
fn test_inherit_true_walks_up() {
    let tmp = TempDir::new().unwrap();
    let child = tmp.path().join("child");
    std::fs::create_dir(&child).unwrap();

    // Parent spec
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
inherit: false
layers:
  - parent-layer
"#,
    );

    // Child spec with inherit: true
    create_spec_file(
        &child,
        r#"
api: spenv/v0
inherit: true
layers:
  - child-layer
"#,
    );

    let options = DiscoveryOptions::default();
    let specs = discover_specs(&child, &options).expect("Should discover specs");

    // Should find both specs
    assert_eq!(specs.len(), 2);
    // Parent comes first in composition order
    assert_eq!(specs[0].layers, vec!["parent-layer"]);
    assert_eq!(specs[1].layers, vec!["child-layer"]);
}

#[rstest]
fn test_force_inherit_option() {
    let tmp = TempDir::new().unwrap();
    let child = tmp.path().join("child");
    std::fs::create_dir(&child).unwrap();

    // Parent spec
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
layers:
  - parent-layer
"#,
    );

    // Child spec with inherit: false (default)
    create_spec_file(
        &child,
        r#"
api: spenv/v0
layers:
  - child-layer
"#,
    );

    let options = DiscoveryOptions {
        force_inherit: true,
        ..Default::default()
    };
    let specs = discover_specs(&child, &options).expect("Should discover specs");

    // Should find both specs due to force_inherit
    assert_eq!(specs.len(), 2);
}

#[rstest]
fn test_no_inherit_option() {
    let tmp = TempDir::new().unwrap();
    let child = tmp.path().join("child");
    std::fs::create_dir(&child).unwrap();

    // Parent spec
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
layers:
  - parent-layer
"#,
    );

    // Child spec with inherit: true
    create_spec_file(
        &child,
        r#"
api: spenv/v0
inherit: true
layers:
  - child-layer
"#,
    );

    let options = DiscoveryOptions {
        no_inherit: true,
        ..Default::default()
    };
    let specs = discover_specs(&child, &options).expect("Should discover spec");

    // Should only find child spec due to no_inherit
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].layers, vec!["child-layer"]);
}

#[rstest]
fn test_not_found_error() {
    let tmp = TempDir::new().unwrap();

    let options = DiscoveryOptions::default();
    let result = discover_specs(tmp.path(), &options);

    assert!(result.is_err());
    match result {
        Err(crate::Error::NotFoundInTree(_)) => {}
        other => panic!("Expected NotFoundInTree, got: {:?}", other),
    }
}

#[rstest]
fn test_local_override() {
    let tmp = TempDir::new().unwrap();

    // Main spec
    create_spec_file(
        tmp.path(),
        r#"
api: spenv/v0
layers:
  - main-layer
"#,
    );

    // Local override
    let local_path = tmp.path().join(SPENV_LOCAL_FILENAME);
    std::fs::write(
        local_path,
        r#"
api: spenv/v0
layers:
  - local-layer
"#,
    )
    .unwrap();

    let options = DiscoveryOptions::default();
    let specs = discover_specs(tmp.path(), &options).expect("Should discover specs");

    // Should find both main and local
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].layers, vec!["main-layer"]);
    assert_eq!(specs[1].layers, vec!["local-layer"]);
}
