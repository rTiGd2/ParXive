use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VolumeHeaderBin {
    pub k: u32,
    pub m: u32,
    pub chunk_size: u32,
    pub total_chunks: u64,
    pub volume_id: u32,
    pub entries_len: u32,
    pub manifest_hash: [u8; 32],
}

/// V2 entry (PARXBV2): adds `outer_for_stripe` to indicate outer RS shard.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VolumeEntry {
    pub stripe: u32,     // inner parity: stripe index; outer parity: u32::MAX
    pub parity_idx: u16, // for inner: 0..m-1; for outer: 0..outer_m-1
    pub offset: u64,
    pub len: u32,
    pub hash: Option<[u8; 32]>,
    pub outer_for_stripe: Option<u32>, // Some(stripe) when this is parity-of-parity shard for that stripe
}

/// V1 entry (PARXBV1): no `outer_for_stripe` field.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VolumeEntryV1 {
    pub stripe: u32,
    pub parity_idx: u16,
    pub offset: u64,
    pub len: u32,
    pub hash: Option<[u8; 32]>,
}

impl From<VolumeEntryV1> for VolumeEntry {
    fn from(v1: VolumeEntryV1) -> Self {
        VolumeEntry {
            stripe: v1.stripe,
            parity_idx: v1.parity_idx,
            offset: v1.offset,
            len: v1.len,
            hash: v1.hash,
            outer_for_stripe: None,
        }
    }
}

/// Try decoding V2; if that fails, fall back to V1 and map.
pub fn decode_entries_anyver(data: &[u8]) -> Result<Vec<VolumeEntry>, bincode::Error> {
    if let Ok(v2) = bincode::deserialize::<Vec<VolumeEntry>>(data) {
        return Ok(v2);
    }
    let v1s: Vec<VolumeEntryV1> = bincode::deserialize(data)?;
    Ok(v1s.into_iter().map(VolumeEntry::from).collect())
}

/// Utility: standard volume filename for id
pub fn vol_name(id: usize) -> String {
    format!("vol-{:03}.parxv", id)
}
