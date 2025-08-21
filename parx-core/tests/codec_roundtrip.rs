use parx_core::rs_codec::RsCodec;
use rand::{rngs::StdRng, Rng, SeedableRng};

#[test]
fn rs_reconstruct_exact_missing_under_m() {
    let mut rng = StdRng::seed_from_u64(42);
    let k = 8usize;
    let m = 4usize;
    let chunk = 32 * 1024;
    let total = k + m;

    // Make shards
    let mut shards: Vec<Vec<u8>> = (0..total)
        .map(|_| (0..chunk).map(|_| rng.gen()).collect())
        .collect();

    // Encode parity over zeroed parity slots
    for i in k..total {
        shards[i].fill(0);
    }
    let mut refs: Vec<&mut [u8]> = shards.iter_mut().map(|v| v.as_mut_slice()).collect();
    RsCodec::new(k, m).unwrap().encode(&mut refs).unwrap();

    // Knock out ≤ m data shards
    let missing = vec![1usize, 3usize, 7usize]; // 3 ≤ m
    let mut opts: Vec<Option<Vec<u8>>> = shards.iter().cloned().map(Some).collect();
    for &i in &missing {
        opts[i] = None;
    }
    RsCodec::new(k, m).unwrap().reconstruct(&mut opts).unwrap();

    // Compare restored vs original
    for &i in &missing {
        assert_eq!(opts[i].as_ref().unwrap(), &shards[i]);
    }
}

