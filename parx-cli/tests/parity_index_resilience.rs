use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::io::{Seek, SeekFrom, Write};
use std::process::Command;

#[test]
fn corrupt_index_trailer_does_not_panic() {
    let td = assert_fs::TempDir::new().unwrap();
    let data = td.child("data");
    data.create_dir_all().unwrap();
    // small dataset
    let mut rng = StdRng::seed_from_u64(99);
    for name in ["x", "y", "z"] {
        let buf: Vec<u8> = (0..(32 * 1024)).map(|_| rng.gen()).collect();
        std::fs::write(data.child(format!("{name}.bin")).path(), buf).unwrap();
    }

    // create with 3 volumes
    Command::cargo_bin("parx")
        .unwrap()
        .current_dir(td.path())
        .args([
            "create",
            "--parity",
            "50",
            "--stripe-k",
            "8",
            "--chunk-size",
            "32768",
            "--output",
            ".parx",
            "--volume-sizes",
            "1M,1M,1M",
            "--gpu",
            "off",
            data.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    // pick a volume and corrupt last 8 KiB (trailer)
    let mut vols: Vec<_> = std::fs::read_dir(td.child(".parx").path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|s| s == "parxv").unwrap_or(false))
        .collect();
    vols.sort();
    let vol = vols.last().expect("at least one volume").to_path_buf();
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&vol).unwrap();
    let len = f.metadata().unwrap().len();
    let tail = 8 * 1024;
    if len > tail + 4 {
        f.seek(SeekFrom::Start(len - tail)).unwrap();
        let junk = vec![0xA5u8; tail as usize];
        f.write_all(&junk).unwrap();
    }

    // quickcheck should not fail; paritycheck should print a summary
    Command::cargo_bin("parx")
        .unwrap()
        .current_dir(td.path())
        .args(["quickcheck", ".parx"])
        .assert()
        .success();

    Command::cargo_bin("parx")
        .unwrap()
        .current_dir(td.path())
        .args(["paritycheck", ".parx"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parity audit across"));
}
