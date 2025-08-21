use blake3::hash;
use parx_core::manifest::ChunkRef;
use parx_core::volume::VolumeEntry;

// Lightweight sanity: deterministic hex helper & types still round-trip via bincode
#[test]
fn volume_entry_bincode_roundtrip() {
    let e = VolumeEntry {
        stripe: 123,
        parity_idx: 2,
        offset: 4096,
        len: 65536,
        hash: Some(*hash(b"xyz").as_bytes()),
    };
    let bin = bincode::serialize(&e).unwrap();
    let de: VolumeEntry = bincode::deserialize(&bin).unwrap();
    assert_eq!(de.stripe, 123);
    assert_eq!(de.parity_idx, 2);
    assert_eq!(de.offset, 4096);
    assert_eq!(de.len, 65536);
    assert_eq!(de.hash.unwrap(), *hash(b"xyz").as_bytes());
}

#[test]
fn chunkref_json_roundtrip() {
    let c = ChunkRef { idx: 7, file_offset: 1024, len: 2048, hash_hex: "abcd".into() };
    let s = serde_json::to_string(&c).unwrap();
    let d: ChunkRef = serde_json::from_str(&s).unwrap();
    assert_eq!(d.idx, 7);
    assert_eq!(d.file_offset, 1024);
    assert_eq!(d.len, 2048);
    assert_eq!(d.hash_hex, "abcd");
}
