use parx_core::merkle;

#[test]
fn merkle_empty_and_single() {
    // Empty -> hash of empty slice
    let root_empty = merkle::root(&[]);
    assert_eq!(root_empty, blake3::hash(&[]));

    // Single leaf -> leaf hash
    let h = blake3::hash(b"alpha");
    let root_single = merkle::root(&[h]);
    assert_eq!(root_single, h);
}

#[test]
fn merkle_pair_and_triplet() {
    let a = blake3::hash(b"a");
    let b = blake3::hash(b"b");
    let c = blake3::hash(b"c");

    // Pair: H(H(a)||H(b))
    let mut ab = [0u8; 64];
    ab[..32].copy_from_slice(a.as_bytes());
    ab[32..].copy_from_slice(b.as_bytes());
    let expect_ab = blake3::hash(&ab);
    assert_eq!(merkle::root(&[a, b]), expect_ab);

    // Triplet: H(H(ab)||H(ab_lastdup))
    let root3 = merkle::root(&[a, b, c]);
    // Just sanity: should be deterministic and not equal to pair root
    assert_ne!(root3, expect_ab);
}
