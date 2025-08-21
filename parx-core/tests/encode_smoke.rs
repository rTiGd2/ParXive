use parx_core::encode::{Encoder, EncoderConfig};
use parx_core::index;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};

#[test]
fn encode_small_dataset_and_verify_manifest_merkle() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path().join("data");
    fs::create_dir(&root).unwrap();
    // Create a few files
    fs::write(root.join("a.bin"), vec![1u8; 10_000]).unwrap();
    fs::write(root.join("b.bin"), vec![2u8; 7_000]).unwrap();

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

    // Manifest file exists
    let mpath = out.join("manifest.json");
    assert!(mpath.exists());

    // Recompute per-chunk hashes and Merkle root to check integrity
    let mut hashes = Vec::new();
    for fe in &manifest.files {
        let mut f = File::open(root.join(&fe.rel_path)).unwrap();
        for ch in &fe.chunks {
            let mut buf = vec![0u8; manifest.chunk_size];
            f.seek(SeekFrom::Start(ch.file_offset)).unwrap();
            // read original len, pad zeros to chunk_size
            let mut tmp = vec![0u8; ch.len as usize];
            f.read_exact(&mut tmp).unwrap();
            buf[..tmp.len()].copy_from_slice(&tmp);
            let h = blake3::hash(&buf);
            hashes.push(h);
            assert_eq!(h.to_hex().to_string(), ch.hash_hex);
        }
    }
    let merkle = parx_core::merkle::root(&hashes).to_hex().to_string();
    assert_eq!(manifest.merkle_root_hex, merkle);

    // Sanity check volume indices parse
    let vols: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|s| s == "parxv").unwrap_or(false))
        .collect();
    assert_eq!(vols.len(), cfg.volumes);
    for p in vols {
        let mut f = File::open(&p).unwrap();
        let (off, len, crc) = index::read_trailer(&mut f).unwrap();
        let count =
            index::read_index_count(&mut f, off, len, crc, &index::IndexLimits::default()).unwrap();
        assert!(count > 0);
    }
}
