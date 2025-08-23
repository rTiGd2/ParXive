use blake3;

/// Compute a simple binary Merkle root over BLAKE3 leaf hashes.
/// Duplicates the last node when the layer is odd.
pub fn root(hashes: &[blake3::Hash]) -> blake3::Hash {
    if hashes.is_empty() {
        return blake3::hash(&[]);
    }
    let mut layer: Vec<[u8; 32]> = hashes.iter().map(|h| *h.as_bytes()).collect();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        let mut i = 0;
        while i < layer.len() {
            let a = layer[i];
            let b = if i + 1 < layer.len() { layer[i + 1] } else { layer[i] };
            let mut cat = [0u8; 64];
            cat[..32].copy_from_slice(&a);
            cat[32..].copy_from_slice(&b);
            next.push(*blake3::hash(&cat).as_bytes());
            i += 2;
        }
        layer = next;
    }
    blake3::Hash::from(layer[0])
}
