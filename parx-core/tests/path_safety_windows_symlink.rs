#![cfg(windows)]

use parx_core::path_safety::{validate_path, PathPolicy};
use std::path::PathBuf;

#[test]
fn reject_symlink_components_when_not_following_windows() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("root");
    let target = root.join("dir");
    std::fs::create_dir_all(&target).unwrap();
    // Try to create a directory symlink: requires Developer Mode or admin.
    let link = root.join("linkdir");
    match std::os::windows::fs::symlink_dir(&target, &link) {
        Ok(()) => {
            // Now refer to linkdir/afile.txt in manifest-like rel path
            let rel = PathBuf::from("linkdir\\afile.txt");
            // Create a file to make canonicalization succeed if we were to follow
            std::fs::write(target.join("afile.txt"), b"hi").unwrap();
            // Policy: do not follow; should reject symlink component
            let res = validate_path(&root, &rel, PathPolicy { follow_symlinks: false });
            assert!(res.is_err(), "expected rejection for symlink component");
        }
        Err(e) => {
            eprintln!("skipping symlink creation test: {}", e);
        }
    }
}
