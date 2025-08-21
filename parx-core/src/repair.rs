use crate::index::{read_index, read_trailer, IndexLimits};
use crate::manifest::Manifest;
use crate::rs_codec::RsCodec;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RepairReport {
    pub repaired_chunks: u64,
    pub failed_chunks: u64,
}

fn collect_parity_shards(
    parity_dir: &Path,
    chunk_size: usize,
) -> Result<HashMap<u32, Vec<Vec<u8>>>> {
    let mut map: HashMap<u32, Vec<Vec<u8>>> = HashMap::new();
    if !parity_dir.exists() {
        return Ok(map);
    }
    for ent in std::fs::read_dir(parity_dir)? {
        let p = ent?.path();
        if p.extension().map(|s| s == "parxv").unwrap_or(false) {
            let mut f = File::open(&p)?;
            let (off, len, crc) = read_trailer(&mut f)?;
            let entries = read_index(&mut f, off, len, crc, &IndexLimits::default())?;
            for e in entries {
                let mut buf = vec![0u8; e.len as usize];
                f.seek(SeekFrom::Start(e.offset))?;
                f.read_exact(&mut buf)?;
                // Ensure full chunk_size for RS
                if buf.len() < chunk_size {
                    buf.resize(chunk_size, 0);
                }
                map.entry(e.stripe).or_default().push(buf);
            }
        }
    }
    Ok(map)
}

/// Attempt repair by reconstructing corrupted chunks using available data and parity shards.
pub fn repair(manifest_path: &Path, root: &Path) -> Result<RepairReport> {
    let mf: Manifest =
        serde_json::from_reader(File::open(manifest_path)?).context("read manifest.json")?;
    let k = mf.stripe_k;
    let m = (mf.stripe_k as u64 * mf.parity_pct as u64).div_ceil(100) as usize;
    if m == 0 {
        bail!("no parity available (parity_pct=0)");
    }
    let rs = RsCodec::new(k, m).context("init RS")?;
    let parity_map = collect_parity_shards(Path::new(&mf.parity_dir), mf.chunk_size)?;

    // Build a map from global chunk idx to (file path, file offset, len)
    let mut idx_map: HashMap<u64, (PathBuf, u64, u32)> = HashMap::new();
    for fe in &mf.files {
        for ch in &fe.chunks {
            idx_map.insert(ch.idx, (root.join(&fe.rel_path), ch.file_offset, ch.len));
        }
    }

    // Verify each chunk; collect stripes that need repair
    let mut to_repair: HashMap<u64, Vec<usize>> = HashMap::new(); // stripe -> missing data indices
    for (&idx, (path, off, len)) in &idx_map {
        let mut f = match File::open(path) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let mut buf = vec![0u8; mf.chunk_size];
        if f.seek(SeekFrom::Start(*off)).is_ok() {
            let mut small = vec![0u8; *len as usize];
            if f.read_exact(&mut small).is_ok() {
                buf[..small.len()].copy_from_slice(&small);
            }
        }
        let h = blake3::hash(&buf);
        // Find expected hash from manifest
        // (We don't have a direct index; compute from known map)
        let expected_hex = mf
            .files
            .iter()
            .flat_map(|fe| fe.chunks.iter())
            .find(|c| c.idx == idx)
            .map(|c| c.hash_hex.clone())
            .unwrap_or_default();
        if h.to_hex().to_string() != expected_hex {
            let stripe = idx / k as u64;
            let data_i = (idx % k as u64) as usize;
            to_repair.entry(stripe).or_default().push(data_i);
        }
    }

    let mut repaired_chunks = 0u64;
    let mut failed_chunks = 0u64;
    for (stripe, data_missing) in to_repair {
        // Collect K data shards
        let mut data_bufs: Vec<Option<Vec<u8>>> = Vec::with_capacity(k);
        for i in 0..k {
            let idx = stripe * k as u64 + i as u64;
            if data_missing.contains(&i) {
                data_bufs.push(None);
            } else {
                // read existing data
                let mut buf = vec![0u8; mf.chunk_size];
                if let Some((path, off, len)) = idx_map.get(&idx) {
                    if let Ok(mut f) = File::open(path) {
                        let _ = f.seek(SeekFrom::Start(*off));
                        let mut small = vec![0u8; *len as usize];
                        if f.read_exact(&mut small).is_ok() {
                            buf[..small.len()].copy_from_slice(&small);
                        }
                    }
                }
                data_bufs.push(Some(buf));
            }
        }
        // Collect M parity shards
        let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(k + m);
        shards.extend(data_bufs);
        let mut parity = Vec::new();
        if let Some(v) = parity_map.get(&(stripe as u32)) {
            parity = v.clone();
        }
        // If insufficient parity shards, skip
        if parity.len() < m {
            failed_chunks += data_missing.len() as u64;
            continue;
        }
        for pbuf in parity.iter().take(m) {
            shards.push(Some(pbuf.clone()));
        }
        // Reconstruct
        if rs.reconstruct(&mut shards).is_ok() {
            // Write back repaired chunks
            for i in data_missing {
                let idx = stripe * k as u64 + i as u64;
                if let Some((path, off, len)) = idx_map.get(&idx) {
                    if let Some(Some(buf)) = shards.get(i) {
                        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
                            if f.seek(SeekFrom::Start(*off)).is_ok() {
                                let _ = f.write_all(&buf[..*len as usize]);
                                repaired_chunks += 1;
                            }
                        }
                    }
                }
            }
        } else {
            failed_chunks += 1;
        }
    }

    Ok(RepairReport { repaired_chunks, failed_chunks })
}
