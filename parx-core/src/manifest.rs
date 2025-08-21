use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub rel_path: String,
    pub size: u64,
    pub chunks: Vec<ChunkRef>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkRef {
    pub idx: u64,
    pub file_offset: u64,
    pub len: u32,
    pub hash_hex: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub created_utc: String,
    pub chunk_size: usize,
    pub stripe_k: usize,
    pub parity_pct: u32,
    pub total_bytes: u64,
    pub total_chunks: u64,
    pub files: Vec<FileEntry>,
    pub merkle_root_hex: String,
    pub parity_dir: String,
    pub volumes: usize,
    pub outer_group: usize,
    pub outer_parity: usize,
}
