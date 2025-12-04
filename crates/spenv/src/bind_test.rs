// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use crate::bind::BindMount;

#[test]
fn test_to_live_layer_bind_relative() {
    // Use a temporary directory so the bind source actually exists.
    let tmp = tempfile::TempDir::new().unwrap();
    let spec_dir = tmp.path();
    let src_dir = spec_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let bm = BindMount {
        bind: "src".to_string(),
        dest: "/spfs/project/src".to_string(),
        readonly: false,
    };

    let ll = bm.to_live_layer_bind(spec_dir).unwrap();
    assert!(ll.src.ends_with("src"));
    assert_eq!(ll.dest, "/spfs/project/src");
}
