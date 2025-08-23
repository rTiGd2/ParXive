use crate::volume::VolumeEntry;
use anyhow::{bail, Context, Result};
use crc32fast::Hasher as Crc32;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

/// Constants for trailer format (index locator at EOF)
const TRAILER_MAGIC: &[u8] = b"PARXINDEX"; // 9 bytes
const TRAILER_LEN: u64 = 9 + 1 + 8 + 4 + 4; // magic + NUL + off + len + crc

/// Index descriptor placed immediately before the compressed index payload.
/// Format: magic (9) + NUL (1) + schema_version (u32 LE) + codec_id (u32 LE) + flags (u32 LE)
const INDEX_DESC_MAGIC: &[u8] = b"PARXIDXD"; // 8 bytes
const INDEX_DESC_LEN: usize = INDEX_DESC_MAGIC.len() + 1 + 4 + 4 + 4; // magic + NUL + schema + codec + flags

/// Optional manifest-backup TLV written just before the trailer.
/// TLV layout: magic (8) + NUL (1) + off(u64) + len(u32) + crc(u32)
const MB_TLV_MAGIC: &[u8] = b"PARXMBTL"; // 8 bytes
const MB_TLV_LEN: usize = 8 + 1 + 8 + 4 + 4; // magic + NUL + off + len + crc

#[derive(Clone, Copy, Debug)]
pub struct ManifestBackupMeta {
    pub off: u64,
    pub len: u32,
    pub crc32: u32,
}

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
pub fn write_index_and_trailer(
    mut f: &File,
    entries: &[VolumeEntry],
    manifest_backup: Option<ManifestBackupMeta>,
) -> Result<()> {
    // Serialize entries
    let raw = bincode::serialize(entries).context("serialize index")?;
    // Compress index payload
    let compressed = zstd::stream::encode_all(&raw[..], 0).context("zstd compress index")?;
    // Build descriptor
    let mut desc = Vec::with_capacity(INDEX_DESC_LEN);
    desc.extend_from_slice(INDEX_DESC_MAGIC);
    desc.push(0);
    desc.extend_from_slice(&1u32.to_le_bytes()); // schema_version = 1
    desc.extend_from_slice(&1u32.to_le_bytes()); // codec_id: 1 = zstd
    desc.extend_from_slice(&0u32.to_le_bytes()); // flags
                                                 // Payload = [desc][compressed]
    let idx_len = (desc.len() + compressed.len()) as u32;
    let idx_off = f.metadata()?.len();
    // CRC over full payload
    let mut h = Crc32::new();
    h.update(&desc);
    h.update(&compressed);
    let crc = h.finalize();
    // Append payload
    f.seek(SeekFrom::End(0))?;
    f.write_all(&desc)?;
    f.write_all(&compressed)?;
    // Optional manifest-backup TLV
    if let Some(mb) = manifest_backup {
        let mut tlv = Vec::with_capacity(MB_TLV_LEN);
        tlv.extend_from_slice(MB_TLV_MAGIC);
        tlv.push(0);
        tlv.extend_from_slice(&mb.off.to_le_bytes());
        tlv.extend_from_slice(&mb.len.to_le_bytes());
        tlv.extend_from_slice(&mb.crc32.to_le_bytes());
        f.write_all(&tlv)?;
    }
    // Trailer (fixed-size tail at EOF)
    let mut tr = Vec::with_capacity(TRAILER_LEN as usize);
    tr.extend_from_slice(TRAILER_MAGIC);
    tr.push(0);
    tr.extend_from_slice(&idx_off.to_le_bytes());
    tr.extend_from_slice(&idx_len.to_le_bytes());
    tr.extend_from_slice(&crc.to_le_bytes());
    f.write_all(&tr)?;
    Ok(())
}

/// Write a compressed (zstd) manifest backup blob, followed by a small footer describing it.
/// The footer is written immediately after the blob. The EOF index trailer remains at end of file.
// Removed legacy write_manifest_backup helper; manifest backup is now referenced via a TLV before the trailer.

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
    // Detect and skip descriptor if present
    let mut start = 0usize;
    if buf.len() >= INDEX_DESC_LEN
        && &buf[..INDEX_DESC_MAGIC.len()] == INDEX_DESC_MAGIC
        && buf[INDEX_DESC_MAGIC.len()] == 0
    {
        start = INDEX_DESC_LEN;
        // Optionally, we could validate schema/codec/flags here
    }
    // Decompress with a guard on output size
    let decompressed = zstd::stream::decode_all(&buf[start..]).context("zstd decompress index")?;
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

/// Scan immediately before the trailer for a manifest-backup TLV and return its metadata if present.
pub fn read_manifest_backup_meta(f: &mut File) -> Result<Option<ManifestBackupMeta>> {
    let flen = f.metadata()?.len();
    if flen < TRAILER_LEN as u64 {
        return Ok(None);
    }
    // Read trailer to confirm it's ours and determine where TLVs would start
    f.seek(SeekFrom::Start(flen - TRAILER_LEN))?;
    let mut tr = vec![0u8; TRAILER_LEN as usize];
    f.read_exact(&mut tr)?;
    if &tr[0..9] != TRAILER_MAGIC || tr[9] != 0 {
        return Ok(None);
    }
    // TLV (if any) starts right before the trailer and has fixed MB_TLV_LEN
    if flen < TRAILER_LEN + MB_TLV_LEN as u64 {
        return Ok(None);
    }
    let tlv_off = flen - TRAILER_LEN - MB_TLV_LEN as u64;
    f.seek(SeekFrom::Start(tlv_off))?;
    let mut tlv = vec![0u8; MB_TLV_LEN];
    f.read_exact(&mut tlv)?;
    if &tlv[..MB_TLV_MAGIC.len()] != MB_TLV_MAGIC || tlv[MB_TLV_MAGIC.len()] != 0 {
        return Ok(None);
    }
    let mut off8 = [0u8; 8];
    off8.copy_from_slice(&tlv[MB_TLV_MAGIC.len() + 1..MB_TLV_MAGIC.len() + 1 + 8]);
    let mut len4 = [0u8; 4];
    len4.copy_from_slice(&tlv[MB_TLV_MAGIC.len() + 1 + 8..MB_TLV_MAGIC.len() + 1 + 8 + 4]);
    let mut crc4 = [0u8; 4];
    crc4.copy_from_slice(&tlv[MB_TLV_MAGIC.len() + 1 + 8 + 4..MB_TLV_MAGIC.len() + 1 + 8 + 4 + 4]);
    Ok(Some(ManifestBackupMeta {
        off: u64::from_le_bytes(off8),
        len: u32::from_le_bytes(len4),
        crc32: u32::from_le_bytes(crc4),
    }))
}

/// Read and return decompressed manifest-backup JSON bytes if present.
pub fn read_manifest_backup_json(f: &mut File) -> Result<Option<Vec<u8>>> {
    if let Some(m) = read_manifest_backup_meta(f)? {
        let mut buf = vec![0u8; m.len as usize];
        f.seek(SeekFrom::Start(m.off))?;
        f.read_exact(&mut buf)?;
        let mut h = Crc32::new();
        h.update(&buf);
        if h.finalize() != m.crc32 {
            bail!("manifest backup CRC mismatch");
        }
        let json = zstd::stream::decode_all(&buf[..]).context("zstd decompress manifest backup")?;
        return Ok(Some(json));
    }
    Ok(None)
}
