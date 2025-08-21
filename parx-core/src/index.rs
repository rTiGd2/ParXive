use crate::volume::VolumeEntry;
use anyhow::{bail, Context, Result};
use crc32fast::Hasher as Crc32;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

/// Constants for trailer format
const TRAILER_MAGIC: &[u8] = b"PARXINDEX"; // 9 bytes
const TRAILER_LEN: u64 = 9 + 1 + 8 + 4 + 4; // magic + NUL + off + len + crc

#[derive(Clone, Copy, Debug)]
pub struct IndexLimits {
    pub max_uncompressed_bytes: usize,
    pub max_entries: usize,
}

impl Default for IndexLimits {
    fn default() -> Self {
        Self { max_uncompressed_bytes: 32 * 1024 * 1024, max_entries: 5_000_000 }
    }
}

/// Write a compressed (zstd) bincode index at EOF and append a CRC'd trailer.
pub fn write_index_and_trailer(mut f: &File, entries: &[VolumeEntry]) -> Result<()> {
    // Serialize
    let raw = bincode::serialize(entries).context("serialize index")?;
    // Compress with default level; bounded in readers
    let compressed = zstd::stream::encode_all(&raw[..], 0).context("zstd compress index")?;
    let idx_len = compressed.len() as u32;
    let idx_off = f.metadata()?.len();
    // CRC over compressed payload
    let mut h = Crc32::new();
    h.update(&compressed);
    let crc = h.finalize();
    // Append index
    f.seek(SeekFrom::End(0))?;
    f.write_all(&compressed)?;
    // Trailer
    let mut tr = Vec::with_capacity(TRAILER_LEN as usize);
    tr.extend_from_slice(TRAILER_MAGIC);
    tr.push(0);
    tr.extend_from_slice(&idx_off.to_le_bytes());
    tr.extend_from_slice(&idx_len.to_le_bytes());
    tr.extend_from_slice(&crc.to_le_bytes());
    f.write_all(&tr)?;
    Ok(())
}

/// Read trailer at EOF; returns (index_off, index_len, crc32)
pub fn read_trailer(f: &mut File) -> Result<(u64, u32, u32)> {
    let flen = f.metadata()?.len();
    if flen < TRAILER_LEN {
        bail!("too short");
    }
    f.seek(SeekFrom::Start(flen - TRAILER_LEN))?;
    let mut tr = vec![0u8; TRAILER_LEN as usize];
    f.read_exact(&mut tr)?;
    if &tr[0..9] != TRAILER_MAGIC || tr[9] != 0 {
        bail!("bad trailer magic");
    }
    let mut off8 = [0u8; 8];
    off8.copy_from_slice(&tr[10..18]);
    let mut len4 = [0u8; 4];
    len4.copy_from_slice(&tr[18..22]);
    let mut crc4 = [0u8; 4];
    crc4.copy_from_slice(&tr[22..26]);
    Ok((u64::from_le_bytes(off8), u32::from_le_bytes(len4), u32::from_le_bytes(crc4)))
}

/// Verify CRC, decompress, and decode index with limits applied.
pub fn read_index(
    f: &mut File,
    idx_off: u64,
    idx_len: u32,
    crc: u32,
    limits: &IndexLimits,
) -> Result<Vec<VolumeEntry>> {
    let mut buf = vec![0u8; idx_len as usize];
    f.seek(SeekFrom::Start(idx_off))?;
    f.read_exact(&mut buf)?;
    let mut h = Crc32::new();
    h.update(&buf);
    let got = h.finalize();
    if got != crc {
        bail!("index CRC mismatch");
    }
    // Decompress with a guard on output size
    let decompressed = zstd::stream::decode_all(&buf[..]).context("zstd decompress index")?;
    if decompressed.len() > limits.max_uncompressed_bytes {
        bail!("index too large: {} bytes", decompressed.len());
    }
    let entries: Vec<VolumeEntry> =
        bincode::deserialize(&decompressed).context("bincode index decode")?;
    if entries.len() > limits.max_entries {
        bail!("too many index entries");
    }
    Ok(entries)
}

/// Convenience: read and return entry count only.
pub fn read_index_count(
    f: &mut File,
    idx_off: u64,
    idx_len: u32,
    crc: u32,
    limits: &IndexLimits,
) -> Result<usize> {
    let v = read_index(f, idx_off, idx_len, crc, limits)?;
    Ok(v.len())
}
