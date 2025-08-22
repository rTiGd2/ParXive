use std::fs::{self, File};
use std::io::Write;
// no extra imports

#[cfg(target_family = "unix")]
fn symlink_dir<P: AsRef<std::path::Path>, Q: AsRef<std::path::Path>>(
    src: P,
    dst: Q,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[test]
fn verify_rejects_symlink_by_default_allows_with_flag_when_contained() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let out = tmp.path().join("out");
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(&out).unwrap();

    // Create a simple file under target
    let p = root.join("target/file.txt");
    let mut f = File::create(&p).unwrap();
    writeln!(f, "hello").unwrap();

    // Encode manifest (no symlinks included during encode)
    let cfg = parx_core::encode::EncoderConfig {
        chunk_size: 1 << 10,
        stripe_k: 2,
        parity_pct: 50,
        volumes: 1,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let mut manifest = parx_core::encode::Encoder::encode(&root, &out, &cfg).unwrap();

    // Re-point the single file entry to a symlinked path safe/ that targets target/
    // Create the symlink inside root
    let safe = root.join("safe");
    symlink_dir(root.join("target"), &safe).unwrap();

    // Modify manifest rel_path
    assert_eq!(manifest.files.len(), 1);
    manifest.files[0].rel_path = "safe/file.txt".to_string();
    let mpath = out.join("manifest.json");
    let mut mf = File::create(&mpath).unwrap();
    mf.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes()).unwrap();

    // Default policy: symlink should be rejected
    let err = parx_core::verify::verify(&mpath, &root).expect_err("expected error");
    let msg = format!("{:#}", err);
    assert!(msg.contains("symlink"), "unexpected error: {}", msg);

    // With follow_symlinks: allowed if contained under root
    let policy = parx_core::path_safety::PathPolicy { follow_symlinks: true };
    let rep = parx_core::verify::verify_with_policy(&mpath, &root, policy).unwrap();
    assert!(rep.merkle_ok);
}

#[test]
fn verify_blocks_symlink_escape_even_when_following() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let out = tmp.path().join("out");
    fs::create_dir_all(root.join("target")).unwrap();
    fs::create_dir_all(&out).unwrap();

    // Real file under root/target
    let p = root.join("target/file.txt");
    let mut f = File::create(&p).unwrap();
    writeln!(f, "hello").unwrap();

    // Encode
    let cfg = parx_core::encode::EncoderConfig {
        chunk_size: 1 << 10,
        stripe_k: 2,
        parity_pct: 50,
        volumes: 1,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let mut manifest = parx_core::encode::Encoder::encode(&root, &out, &cfg).unwrap();

    // Create a symlink that escapes: root/evil -> root/.. (the parent tempdir)
    let parent = root.parent().unwrap();
    let evil = root.join("evil");
    symlink_dir(parent, &evil).unwrap();
    // Place a file outside the root so canonicalization succeeds, then we fail containment
    let outside = parent.join("outside.txt");
    let mut of = File::create(&outside).unwrap();
    writeln!(of, "outside").unwrap();
    // Point manifest to evil/outside.txt (escapes root)
    manifest.files[0].rel_path = "evil/outside.txt".to_string();
    let mpath = out.join("manifest.json");
    let mut mf = File::create(&mpath).unwrap();
    mf.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes()).unwrap();

    let policy = parx_core::path_safety::PathPolicy { follow_symlinks: true };
    let err = parx_core::verify::verify_with_policy(&mpath, &root, policy)
        .expect_err("expected escape error");
    let msg = format!("{:#}", err);
    assert!(msg.contains("escapes root"));
}
