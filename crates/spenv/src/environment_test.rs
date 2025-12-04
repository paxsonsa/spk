// Copyright (c) Contributors to the SPK project.
// SPDX-License-Identifier: Apache-2.0

use crate::environment::{generate_startup_script, get_priority, EnvOp, AppendEnv, CommentEnv, PrependEnv, PriorityEnv, SetEnv};

#[test]
fn test_generate_startup_script_basic() {
    let ops = vec![
        EnvOp::Comment(CommentEnv {
            comment: "Example environment".to_string(),
        }),
        EnvOp::Set(SetEnv {
            set: "FOO".to_string(),
            value: "bar".to_string(),
        }),
        EnvOp::Prepend(PrependEnv {
            prepend: "PATH".to_string(),
            value: "/spfs/bin".to_string(),
            separator: None,
        }),
        EnvOp::Append(AppendEnv {
            append: "LD_LIBRARY_PATH".to_string(),
            value: "/spfs/lib".to_string(),
            separator: Some(":".to_string()),
        }),
    ];

    let script = generate_startup_script(&ops);

    assert!(script.contains("# Example environment"));
    assert!(script.contains("export FOO=\"bar\""));
    assert!(script.contains("export PATH=\"/spfs/bin:"));
    assert!(script.contains("export LD_LIBRARY_PATH=\"${LD_LIBRARY_PATH}:"));
}

#[test]
fn test_escape_and_priority_defaults() {
    let ops = vec![
        EnvOp::Set(SetEnv {
            set: "SPECIAL".to_string(),
            value: "value with $dollar and \"quotes\"".to_string(),
        }),
        EnvOp::Priority(PriorityEnv { priority: 10 }),
    ];

    let script = generate_startup_script(&ops);
    assert!(script.contains("SPECIAL"));
    assert!(!script.contains("$dollar and \"quotes\""));

    let priority = get_priority(&ops);
    assert_eq!(priority, 10);

    let default_priority = get_priority(&[]);
    assert_eq!(default_priority, 50);
}
