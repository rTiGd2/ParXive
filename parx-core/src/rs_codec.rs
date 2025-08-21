use anyhow::Result;
use reed_solomon_erasure::galois_8::ReedSolomon;

pub struct RsCodec {
    pub k: usize,
    pub m: usize,
    inner: ReedSolomon,
}

impl RsCodec {
    pub fn new(k: usize, m: usize) -> Result<Self> {
        let inner = ReedSolomon::new(k, m)?;
        Ok(Self { k, m, inner })
    }

    pub fn encode(&self, shards: &mut [&mut [u8]]) -> Result<()> {
        self.inner.encode(shards)?;
        Ok(())
    }

    // Note: reconstruct expects Option<Vec<u8>> buffers
    pub fn reconstruct(&self, shards: &mut [Option<Vec<u8>>]) -> Result<()> {
        self.inner.reconstruct(shards)?;
        Ok(())
    }
}
