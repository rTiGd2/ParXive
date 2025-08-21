use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::process::Command;

fn write_random(path: &std::path::Path, bytes: usize, seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    let data: Vec<u8> = (0..bytes).map(|_| rng.gen()).collect();
    std::fs::write(path, data).unwrap();
}

#[test]
fn create_verify_repair_happy_path() {
    let td = assert_fs::TempDir::new().unwrap();
    let data = td.child("demo_data");
    data.create_dir_all().unwrap();
    // 3 small files, ~64 KiB each
    write_random(&data.child("a.bin").path(), 64 * 1024, 1);
    write_random(&data.child("b.bin").path(), 64 * 1024, 2);
    write_random(&data.child("c.bin").path(), 64 * 1024, 3);

    // create
    let mut cmd = Command::cargo_bin("parx").unwrap();
    cmd.current_dir(td.path())
        .args([
            "create",
            "--parity", "50",
            "--stripe-k", "8",
            "--chunk-size", "65536",
            "--output", ".parx",
            "--volume-sizes", "2M,2M,2M",
            "--gpu", "off",
            data.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    // verify OK
    Command::cargo_bin("parx").unwrap()
        .current_dir(td.path())
        .args(["verify", ".parx/manifest.json", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK"));

    // Corrupt 4 KiB in one file (should be repairable with 50% parity for k=8 => m=4)
    let fpath = data.child("b.bin").path().to_path_buf();
    {
        use std::io::{Write, Seek, SeekFrom};
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&fpath).unwrap();
        f.seek(SeekFrom::Start(8 * 1024)).unwrap();
        let garbage = vec![0xFFu8; 4096];
        f.write_all(&garbage).unwrap();
    }

    // audit says repairable
    Command::cargo_bin("parx").unwrap()
        .current_dir(td.path())
        .args(["audit", ".parx/manifest.json", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("Repairable: YES"));

    // repair
    Command::cargo_bin("parx").unwrap()
        .current_dir(td.path())
        .args(["repair", ".parx/manifest.json", "."])
        .assert()
        .success();

    // verify OK again
    Command::cargo_bin("parx").unwrap()
        .current_dir(td.path())
        .args(["verify", ".parx/manifest.json", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK"));
}

