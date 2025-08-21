use parx_core::encode::{Encoder, EncoderConfig};
use parx_core::repair;
use parx_core::verify;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

#[test]
fn verify_then_repair_simple_corruption() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    fs::create_dir(&root).unwrap();
    // Two files with distinct bytes
    fs::write(root.join("a.bin"), vec![1u8; 32 * 1024]).unwrap();
    fs::write(root.join("b.bin"), vec![2u8; 32 * 1024]).unwrap();

    let out = td.path().join(".parx");
    let cfg = EncoderConfig {
        chunk_size: 4096,
        stripe_k: 4,
        parity_pct: 50,
        volumes: 2,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let manifest = Encoder::encode(&root, &out, &cfg).unwrap();

    // Verify OK initially
    let vr = verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert_eq!(vr.chunks_bad, 0);
    assert!(vr.merkle_ok);

    // Corrupt 2 KiB in middle of b.bin
    let mut f = OpenOptions::new().read(true).write(true).open(root.join("b.bin")).unwrap();
    f.seek(SeekFrom::Start(8 * 1024)).unwrap();
    f.write_all(&vec![0xA5u8; 2 * 1024]).unwrap();

    // Verify now detects bad chunks
    let vr2 = verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert!(vr2.chunks_bad > 0);

    // Attempt repair
    let rr = repair::repair(&out.join("manifest.json"), &root).unwrap();
    assert!(rr.repaired_chunks >= 1);

    // Verify OK again
    let vr3 = verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert_eq!(vr3.chunks_bad, 0);
    assert!(vr3.merkle_ok);

    // silence unused
    let _ = manifest;
}
