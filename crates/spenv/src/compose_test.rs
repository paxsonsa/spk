// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use rstest::rstest;
use std::path::PathBuf;

use super::*;
use crate::bind::BindMount;
use crate::environment::{EnvOp, PriorityEnv, SetEnv};
use crate::spec::ApiVersion;

fn make_spec(layers: Vec<&str>, source_path: Option<&str>) -> EnvSpec {
    EnvSpec {
        api: ApiVersion::V0,
        description: None,
        inherit: false,
        includes: Vec::new(),
        layers: layers.into_iter().map(String::from).collect(),
        environment: Vec::new(),
        contents: Vec::new(),
        packages: Vec::new(),
        package_options: None,
        source_path: source_path.map(PathBuf::from),
    }
}

#[rstest]
fn test_compose_empty() {
    let specs: Vec<EnvSpec> = vec![];
    let composed = compose_specs(&specs);

    assert!(composed.layers.is_empty());
    assert!(composed.source_files.is_empty());
}

#[rstest]
fn test_compose_single() {
    let specs = vec![make_spec(vec!["layer1", "layer2"], Some("/path/to/spec"))];
    let composed = compose_specs(&specs);

    assert_eq!(composed.layers, vec!["layer1", "layer2"]);
    assert_eq!(composed.source_files, vec![PathBuf::from("/path/to/spec")]);
}

#[rstest]
fn test_compose_multiple() {
    let specs = vec![
        make_spec(vec!["parent-layer"], Some("/parent/.spenv.yaml")),
        make_spec(vec!["child-layer"], Some("/parent/child/.spenv.yaml")),
    ];
    let composed = compose_specs(&specs);

    // Layers are appended in order
    assert_eq!(composed.layers, vec!["parent-layer", "child-layer"]);
    assert_eq!(composed.source_files.len(), 2);
}

#[rstest]
fn test_compose_overlapping_layers() {
    let specs = vec![
        make_spec(vec!["base", "tools"], None),
        make_spec(vec!["dev", "tools"], None), // "tools" appears again
    ];
    let composed = compose_specs(&specs);

    // Layers are appended as-is, deduplication happens at SPFS level
    assert_eq!(composed.layers, vec!["base", "tools", "dev", "tools"]);
}

#[rstest]
fn test_has_layers() {
    let empty = ComposedEnvironment::default();
    assert!(!empty.has_layers());

    let with_layers = compose_specs(&[make_spec(vec!["layer"], None)]);
    assert!(with_layers.has_layers());
}

#[rstest]
fn test_compose_environment_operations() {
    let spec1 = EnvSpec {
        api: ApiVersion::V0,
        description: None,
        inherit: false,
        includes: Vec::new(),
        layers: vec!["base".to_string()],
        environment: vec![EnvOp::Set(SetEnv {
            set: "FOO".to_string(),
            value: "one".to_string(),
        })],
        contents: Vec::new(),
        packages: Vec::new(),
        package_options: None,
        source_path: None,
    };

    let spec2 = EnvSpec {
        api: ApiVersion::V0,
        description: None,
        inherit: false,
        includes: Vec::new(),
        layers: vec!["dev".to_string()],
        environment: vec![EnvOp::Priority(PriorityEnv { priority: 10 })],
        contents: Vec::new(),
        packages: Vec::new(),
        package_options: None,
        source_path: None,
    };

    let composed = compose_specs(&[spec1, spec2]);

    assert_eq!(composed.layers, vec!["base", "dev"]);
    assert_eq!(composed.environment.len(), 2);
}

#[rstest]
fn test_compose_contents() {
    let spec = EnvSpec {
        api: ApiVersion::V0,
        description: None,
        inherit: false,
        includes: Vec::new(),
        layers: vec!["base".to_string()],
        environment: Vec::new(),
        contents: vec![BindMount {
            bind: "./src".to_string(),
            dest: "/spfs/project/src".to_string(),
            readonly: false,
        }],
        packages: Vec::new(),
        package_options: None,
        source_path: Some(PathBuf::from("/project/.spenv.yaml")),
    };

    let composed = compose_specs(&[spec]);
    assert_eq!(composed.contents.len(), 1);
    assert_eq!(composed.contents[0].dest, "/spfs/project/src");
}
