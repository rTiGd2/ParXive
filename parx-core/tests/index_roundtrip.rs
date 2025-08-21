use parx_core::index;
use parx_core::volume::VolumeEntry;
use std::fs::File;
use std::io::Write;

#[test]
fn index_write_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vol-test.parxv");
    let mut f = File::create(&path).unwrap();
    // Write some payload (header placeholder)
    f.write_all(&[0u8; 32]).unwrap();
    let entries = vec![
        VolumeEntry {
            stripe: 0,
            parity_idx: 0,
            offset: 32,
            len: 1024,
            hash: None,
            outer_for_stripe: None,
        },
        VolumeEntry {
            stripe: 1,
            parity_idx: 1,
            offset: 1056,
            len: 1024,
            hash: None,
            outer_for_stripe: None,
        },
    ];
    index::write_index_and_trailer(&f, &entries).unwrap();

    let mut f2 = File::open(&path).unwrap();
    let (off, len, crc) = index::read_trailer(&mut f2).unwrap();
    let out = index::read_index(&mut f2, off, len, crc, &index::IndexLimits::default()).unwrap();
    assert_eq!(out.len(), entries.len());
    assert_eq!(out[0].stripe, 0);
    assert_eq!(out[1].parity_idx, 1);
}
