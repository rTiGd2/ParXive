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

#[test]
fn repair_single_file_bitflip() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    std::fs::create_dir(&root).unwrap();
    // Single file with random data (~8 MiB)
    let path = root.join("single.bin");
    let mut f = std::fs::File::create(&path).unwrap();
    // Deterministic bytes for stability
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    fastrand::seed(0x1BADF00Du64);
    for b in &mut buf {
        *b = fastrand::u8(..);
    }
    std::io::Write::write_all(&mut f, &buf).unwrap();

    let out = td.path().join(".parx");
    let cfg = EncoderConfig {
        chunk_size: 65536, // 64 KiB
        stripe_k: 8,
        parity_pct: 25, // 2 parity shards
        volumes: 3,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let _manifest = Encoder::encode(&root, &out, &cfg).unwrap();

    // Flip a random 4 KiB page
    use std::io::{Seek, SeekFrom, Write};
    let mut g = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let sz = g.metadata().unwrap().len();
    let off = ((fastrand::u64(..(sz - 4096))) / 4096) * 4096; // page aligned
    g.seek(SeekFrom::Start(off)).unwrap();
    let mut flip = vec![0u8; 4096];
    fastrand::seed(0xC0FFEEu64);
    for b in &mut flip {
        *b = fastrand::u8(..);
    }
    g.write_all(&flip).unwrap();

    // Verify now detects bad chunks
    let vr2 = verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert!(vr2.chunks_bad > 0);

    // Attempt repair
    let rr = repair::repair(&out.join("manifest.json"), &root).unwrap();
    assert!(rr.repaired_chunks >= 1, "expected at least one repaired chunk: {:?}", rr);

    // Verify OK again
    let vr3 = verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert_eq!(vr3.chunks_bad, 0);
    assert!(vr3.merkle_ok);
}
