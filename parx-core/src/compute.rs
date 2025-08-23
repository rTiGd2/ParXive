use anyhow::Result;

/// A compute backend that can encode parity for one stripe (K data shards -> M parity shards).
pub trait ComputeBackend {
    /// Encodes one stripe.
    /// - data_shards: K slices of length C each (zero-padded)
    /// - parity_out: M mutable slices of length C each (to be filled)
    fn encode_stripe(&self, data_shards: &[&[u8]], parity_out: &mut [&mut [u8]]) -> Result<()>;

    /// Optional convenience: encode a batch of stripes.
    /// Default implementation loops over stripes and calls encode_stripe.
    fn encode_batch(
        &self,
        data_stripes: &[Vec<&[u8]>],
        parity_out: &mut [Vec<&mut [u8]>],
    ) -> Result<()> {
        for (i, ds) in data_stripes.iter().enumerate() {
            self.encode_stripe(ds, &mut parity_out[i][..])?;
        }
        Ok(())
    }
}

/// CPU implementation using RsCodec.
pub struct CpuBackend {
    k: usize,
    m: usize,
}

impl CpuBackend {
    pub fn new(k: usize, m: usize) -> Result<Self> {
        Ok(Self { k, m })
    }
}

impl ComputeBackend for CpuBackend {
    fn encode_stripe(&self, data_shards: &[&[u8]], parity_out: &mut [&mut [u8]]) -> Result<()> {
        use crate::rs_codec::RsCodec;
        let mut shards: Vec<&mut [u8]> = Vec::with_capacity(self.k + self.m);
        // Safety: we create a local owned vector that holds mutable parity shards; data shards are borrowed read-only
        // For RsCodec we need &mut [u8] for all shards; clone to a temporary owned buffer for data is costly.
        // Instead, construct a contiguous Vec for data+parity copies minimally. For now, create a shadow mutable buffer for data shards.
        let mut data_owned: Vec<Vec<u8>> = Vec::with_capacity(self.k);
        for d in data_shards {
            let mut v = vec![0u8; d.len()];
            v.copy_from_slice(d);
            data_owned.push(v);
        }
        for v in &mut data_owned {
            shards.push(v.as_mut_slice());
        }
        for p in parity_out.iter_mut() {
            shards.push(p.as_mut());
        }
        let rs = RsCodec::new(self.k, self.m)?;
        rs.encode(&mut shards[..])
    }
}
