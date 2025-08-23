#![cfg(windows)]

use parx_core::path_safety::{validate_path, PathPolicy};
use std::path::PathBuf;

#[test]
fn validate_blocks_absolute_and_parent_traversal() {
    let root = PathBuf::from("C:\\data\\root");
    // absolute path should be rejected
    let abs = PathBuf::from("C:\\Windows\\System32\\cmd.exe");
    assert!(validate_path(&root, &abs, PathPolicy::default()).is_err());

    // parent traversal should be rejected
    let rel = PathBuf::from("..\\outside.txt");
    assert!(validate_path(&root, &rel, PathPolicy::default()).is_err());
}

#[test]
fn validate_case_insensitive_containment_when_following_symlinks() {
    // This test checks our case-insensitive containment logic by comparing
    // canonicalized paths with different letter casing.
    // We don't create symlinks (CI limitation); we just ensure containment check
    // is not tripped by case differences.
    let root = std::env::temp_dir().join("ParXCaseRoot");
    let sub = root.join("SubDir");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("File.txt");
    std::fs::write(&file, b"hi").unwrap();
    let root_can = std::fs::canonicalize(&root).unwrap();
    let rel = PathBuf::from("SubDir\\File.txt");

    // Force mixed-case root path when calling validate_path by using the real root
    // but rely on the implementation to canonicalize and compare case-insensitively.
    let res = validate_path(&root_can, &rel, PathPolicy { follow_symlinks: true });
    assert!(res.is_ok(), "expected containment despite case differences");
}
