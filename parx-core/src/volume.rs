use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VolumeEntry {
    /// Stripe number this parity shard belongs to
    pub stripe: u32,
    /// Parity index within the stripe [0..M-1]
    pub parity_idx: u16,
    /// Absolute file offset where the shard bytes begin (from start of .parxv file)
    pub offset: u64,
    /// Shard length in bytes (== chunk_size)
    pub len: u32,
    /// Optional BLAKE3 hash of this parity shard. Absent for old volumes.
    #[serde(default)]
    pub hash: Option<[u8; 32]>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VolumeHeaderBin {
    pub k: u32,
    pub m: u32,
    pub chunk_size: u32,
    pub total_chunks: u64,
    pub volume_id: u32,
    /// NOTE: In v0.4.3+ we repurpose this as *entry_count* (number of parity shards in this volume).
    /// It was previously unused (0). Older volumes will show 0 here.
    pub entries_len: u32,
    pub manifest_hash: [u8; 32],
}

/// Legacy/simple name used during initial creation (before we know counts).
pub fn vol_name(i: usize) -> String {
    format!("vol-{:03}.parxv", i)
}

/// PAR2-style name with block count, e.g. vol-000+022.parxv
pub fn vol_name_with_blocks(i: usize, blocks: usize) -> String {
    format!("vol-{:03}+{:03}.parxv", i, blocks)
}
