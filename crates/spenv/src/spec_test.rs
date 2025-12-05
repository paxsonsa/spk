// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use rstest::rstest;

use super::*;

#[rstest]
fn test_parse_minimal_spec() {
    let yaml = r#"
api: spenv/v0
"#;
    let spec = EnvSpec::from_yaml(yaml).expect("Should parse minimal spec");
    assert_eq!(spec.api, ApiVersion::V0);
    assert!(!spec.inherit); // Default is false
    assert!(spec.layers.is_empty());
    assert!(spec.includes.is_empty());
}

#[rstest]
fn test_parse_full_spec() {
    let yaml = r#"
api: spenv/v0
description: "Test environment"
inherit: true
includes:
  - ~/config/base.spenv.yaml
  - /team/shared.spenv.yaml
layers:
  - platform/centos7
  - dev-tools/latest
"#;
    let spec = EnvSpec::from_yaml(yaml).expect("Should parse full spec");
    assert_eq!(spec.api, ApiVersion::V0);
    assert_eq!(spec.description, Some("Test environment".to_string()));
    assert!(spec.inherit);
    assert_eq!(spec.includes.len(), 2);
    assert_eq!(spec.layers.len(), 2);
    assert_eq!(spec.layers[0], "platform/centos7");
    assert_eq!(spec.layers[1], "dev-tools/latest");
}

#[rstest]
fn test_parse_spec_with_environment_ops() {
    let yaml = r#"
api: spenv/v0
layers:
  - base
environment:
  - set: FOO
    value: bar
  - prepend: PATH
    value: /spfs/bin
  - append: LD_LIBRARY_PATH
    value: /spfs/lib
  - comment: "example comment"
  - priority: 10
"#;

    let spec = EnvSpec::from_yaml(yaml).expect("Should parse spec with environment ops");
    assert_eq!(spec.layers, vec!["base"]);
    assert_eq!(spec.environment.len(), 5);
}

#[rstest]
fn test_parse_spec_with_packages() {
    let yaml = r#"
api: spenv/v0
packages:
  - python/3.11
  - cmake/3.26
package_options:
  binary_only: true
  repositories:
    - default
  solver: resolvo
"#;

    let spec = EnvSpec::from_yaml(yaml).expect("Should parse spec with packages");
    assert_eq!(spec.packages, vec!["python/3.11", "cmake/3.26"]);
    let opts = spec
        .package_options
        .expect("package_options should be present");
    assert!(opts.binary_only);
    assert_eq!(opts.repositories, vec!["default"]);
    assert_eq!(opts.solver.as_deref(), Some("resolvo"));
}

#[rstest]
fn test_inherit_defaults_to_false() {
    let yaml = r#"
api: spenv/v0
layers:
  - some-layer
"#;
    let spec = EnvSpec::from_yaml(yaml).expect("Should parse spec");
    assert!(
        !spec.inherit,
        "inherit should default to false for security"
    );
}

#[rstest]
fn test_parse_invalid_yaml() {
    let yaml = r#"
api: spenv/v0
layers: [
  unclosed bracket
"#;
    let result = EnvSpec::from_yaml(yaml);
    assert!(result.is_err(), "Should fail on invalid YAML");
}

#[rstest]
fn test_default_spec() {
    let spec = EnvSpec::default();
    assert_eq!(spec.api, ApiVersion::V0);
    assert!(!spec.inherit);
    assert!(spec.layers.is_empty());
    assert!(spec.includes.is_empty());
    assert!(spec.packages.is_empty());
    assert!(spec.package_options.is_none());
    assert!(spec.source_path.is_none());
}
