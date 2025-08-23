use parx_core::encode::{Encoder, EncoderConfig};
use parx_core::index::{read_index, read_trailer, IndexLimits};
use parx_core::manifest::Manifest;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[test]
fn chunk_hash_zero_padding_matches() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    std::fs::create_dir(&root).unwrap();
    // Make a file that is not a multiple of chunk size
    let chunk_size = 4096usize;
    let data_len = chunk_size + (chunk_size / 2);
    let mut buf = vec![0u8; data_len];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(31).wrapping_add(7);
    }
    std::fs::write(root.join("file.bin"), &buf).unwrap();

    let out = td.path().join(".parx");
    let cfg = EncoderConfig {
        chunk_size,
        stripe_k: 4,
        parity_pct: 50,
        volumes: 2,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let _manifest = Encoder::encode(&root, &out, &cfg).unwrap();

    // Recompute hashes directly from source with zero-padding and compare
    let mf: Manifest =
        serde_json::from_reader(File::open(out.join("manifest.json")).unwrap()).unwrap();
    for fe in &mf.files {
        let mut f = File::open(root.join(&fe.rel_path)).unwrap();
        for ch in &fe.chunks {
            let mut chunk = vec![0u8; mf.chunk_size];
            f.seek(SeekFrom::Start(ch.file_offset)).unwrap();
            if ch.len > 0 {
                f.read_exact(&mut chunk[..ch.len as usize]).unwrap();
            }
            let h = blake3::hash(&chunk);
            assert_eq!(h.to_hex().to_string(), ch.hash_hex);
        }
    }
}

#[test]
fn parity_entry_len_within_bounds() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    std::fs::create_dir(&root).unwrap();
    // Multiple files to ensure multi-stripe
    std::fs::write(root.join("a.bin"), vec![1u8; 32 * 1024]).unwrap();
    std::fs::write(root.join("b.bin"), vec![2u8; 48 * 1024]).unwrap();

    let out = td.path().join(".parx");
    let cfg = EncoderConfig {
        chunk_size: 4096,
        stripe_k: 4,
        parity_pct: 50,
        volumes: 3,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let _manifest = Encoder::encode(&root, &out, &cfg).unwrap();

    // Inspect volume indices and ensure entry.len <= chunk_size and > 0
    for vid in 0..cfg.volumes {
        let path = out.join(format!("vol-{vid:03}.parxv"));
        let mut f = File::open(&path).unwrap();
        let (off, len, crc) = read_trailer(&mut f).unwrap();
        let entries = read_index(&mut f, off, len, crc, &IndexLimits::default()).unwrap();
        for e in entries {
            assert!(e.len as usize <= cfg.chunk_size);
            assert!(e.len > 0);
        }
    }
}

#[test]
fn interleave_preserves_order_and_hashes() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    std::fs::create_dir(&root).unwrap();
    // Make three files with distinct content
    std::fs::write(root.join("a.bin"), vec![1u8; 10 * 1024]).unwrap();
    std::fs::write(root.join("b.bin"), vec![2u8; 10 * 1024]).unwrap();
    std::fs::write(root.join("c.bin"), vec![3u8; 10 * 1024]).unwrap();

    let out_seq = td.path().join(".parx_seq");
    let out_il = td.path().join(".parx_il");
    let cfg_seq = EncoderConfig {
        chunk_size: 4096,
        stripe_k: 2,
        parity_pct: 50,
        volumes: 2,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let cfg_il = EncoderConfig {
        chunk_size: 4096,
        stripe_k: 2,
        parity_pct: 50,
        volumes: 2,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: true,
    };
    let _m1 = Encoder::encode(&root, &out_seq, &cfg_seq).unwrap();
    let _m2 = Encoder::encode(&root, &out_il, &cfg_il).unwrap();

    // Verify both pass
    let v1 = parx_core::verify::verify(&out_seq.join("manifest.json"), &root).unwrap();
    assert_eq!(v1.chunks_bad, 0);
    assert!(v1.merkle_ok);
    let v2 = parx_core::verify::verify(&out_il.join("manifest.json"), &root).unwrap();
    assert_eq!(v2.chunks_bad, 0);
    assert!(v2.merkle_ok);
}

#[test]
fn multi_stripe_repair_succeeds() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    std::fs::create_dir(&root).unwrap();
    // Create data to span several stripes
    std::fs::write(root.join("big.bin"), vec![9u8; 256 * 1024]).unwrap();
    let out = td.path().join(".parx");
    let cfg = EncoderConfig {
        chunk_size: 4096,
        stripe_k: 4,
        parity_pct: 50,
        volumes: 3,
        outer_group: 0,
        outer_parity: 0,
        interleave_files: false,
    };
    let _m = Encoder::encode(&root, &out, &cfg).unwrap();

    // Corrupt multiple regions across stripes
    use std::io::{Seek, SeekFrom, Write};
    let mut f =
        std::fs::OpenOptions::new().read(true).write(true).open(root.join("big.bin")).unwrap();
    for off in [8 * 1024u64, 24 * 1024u64, 40 * 1024u64] {
        f.seek(SeekFrom::Start(off)).unwrap();
        f.write_all(&vec![0x5Au8; 2048]).unwrap();
    }

    // Verify detects
    let vr = parx_core::verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert!(vr.chunks_bad > 0);
    // Repair
    let rr = parx_core::repair::repair(&out.join("manifest.json"), &root).unwrap();
    assert!(rr.repaired_chunks >= 1);
    // Verify OK
    let vr2 = parx_core::verify::verify(&out.join("manifest.json"), &root).unwrap();
    assert_eq!(vr2.chunks_bad, 0);
    assert!(vr2.merkle_ok);
}
