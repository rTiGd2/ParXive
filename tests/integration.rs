use assert_cmd::prelude::*;
use predicates::prelude::*;
use rand::{Rng, SeedableRng};
use std::{fs::{self, File}, io::{Write}, process::Command};

fn write_random_file(path: &std::path::Path, size: usize) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
    let mut f = File::create(path).unwrap();
    let mut buf = vec![0u8; 1<<20];
    let mut remaining = size;
    while remaining > 0 {
        let n = remaining.min(buf.len());
        rng.fill(&mut buf[..n]);
        f.write_all(&buf[..n]).unwrap();
        remaining -= n;
    }
}

#[test]
fn end_to_end_outer_rs_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Data
    let big = root.join("big.bin");
    write_random_file(&big, 40 * (1<<20));
    let parts_dir = root.join("parts");
    fs::create_dir_all(&parts_dir).unwrap();
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.args(["split", big.to_str().unwrap(), parts_dir.to_str().unwrap(), "8"]);
    cmd.assert().success();

    // Create parity (outer RS enabled)
    let parxdir = root.join(".parx");
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.current_dir(&root);
    cmd.args(["create",
        "--parity","35",
        "--stripe-k","64",
        "--chunk-size","1048576",
        "--output", parxdir.to_str().unwrap(),
        "--volume-sizes","5M,10M,20M",
        "--outer-group","128",
        "--outer-parity","2",
        parts_dir.to_str().unwrap()]);
    cmd.assert().success();

    // Verify OK
    let manifest = parxdir.join("manifest.json");
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.args(["verify", manifest.to_str().unwrap(), root.to_str().unwrap()]);
    cmd.assert().success().stdout(predicate::str::contains("OK"));

    // Remove a part, audit, repair, verify
    fs::remove_file(parts_dir.join("part-002.bin")).unwrap();
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.args(["audit", manifest.to_str().unwrap(), root.to_str().unwrap()]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.args(["repair", manifest.to_str().unwrap(), root.to_str().unwrap()]);
    cmd.assert().success();
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.args(["verify", manifest.to_str().unwrap(), root.to_str().unwrap()]);
    cmd.assert().success();
}
