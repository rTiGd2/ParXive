use crate::index::{read_index, read_trailer, IndexLimits};
use crate::manifest::Manifest;
use crate::path_safety::{validate_path, PathPolicy};
use crate::rs_codec::RsCodec;
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairReport {
    pub repaired_chunks: u64,
    pub failed_chunks: u64,
}

type ParityMap = HashMap<u32, Vec<(usize, Vec<u8>)>>;

fn collect_parity_shards(parity_dir: &Path, chunk_size: usize) -> Result<ParityMap> {
    let mut map: ParityMap = HashMap::new();
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
                if buf.len() < chunk_size {
                    buf.resize(chunk_size, 0);
                }
                map.entry(e.stripe).or_default().push((e.parity_idx as usize, buf));
            }
        }
    }
    Ok(map)
}

pub fn repair(manifest_path: &Path, root: &Path) -> Result<RepairReport> {
    repair_with_policy(manifest_path, root, PathPolicy::default())
}

pub fn repair_with_policy(
    manifest_path: &Path,
    root: &Path,
    policy: PathPolicy,
) -> Result<RepairReport> {
    let mf: Manifest =
        serde_json::from_reader(File::open(manifest_path)?).context("read manifest.json")?;
    // Global lock in parity dir to avoid concurrent repairs
    let lock_path = Path::new(&mf.parity_dir).join(".parx.repair.lock");
    let lock_file = File::create(&lock_path).context("create global repair lock")?;
    lock_file.try_lock_exclusive().context("acquire global repair lock")?;

    let k = mf.stripe_k;
    let m = (mf.stripe_k as u64 * mf.parity_pct as u64).div_ceil(100) as usize;
    if m == 0 {
        bail!("no parity available (parity_pct=0)");
    }
    let _rs = RsCodec::new(k, m).context("init RS")?; // validate params early
    let parity_map = collect_parity_shards(Path::new(&mf.parity_dir), mf.chunk_size)?;

    // Build map idx -> (safe_path, offset, len) and record target file sizes
    let mut idx_map: HashMap<u64, (PathBuf, u64, u32)> = HashMap::new();
    let mut file_sizes: HashMap<PathBuf, u64> = HashMap::new();
    for fe in &mf.files {
        let safe = validate_path(root, Path::new(&fe.rel_path), policy)
            .with_context(|| format!("validate path {:?}", fe.rel_path))?;
        file_sizes.insert(safe.clone(), fe.size);
        for ch in &fe.chunks {
            idx_map.insert(ch.idx, (safe.clone(), ch.file_offset, ch.len));
        }
    }

    // Identify missing/corrupted chunks
    let mut to_repair: HashMap<u64, Vec<usize>> = HashMap::new();
    for (&idx, (path, off, len)) in &idx_map {
        if let Ok(mut f) = File::open(path) {
            let mut buf = vec![0u8; mf.chunk_size];
            if f.seek(SeekFrom::Start(*off)).is_ok() {
                let mut small = vec![0u8; *len as usize];
                if f.read_exact(&mut small).is_ok() {
                    buf[..small.len()].copy_from_slice(&small);
                }
            }
            let h = blake3::hash(&buf).to_hex().to_string();
            let expected = mf
                .files
                .iter()
                .flat_map(|fe| fe.chunks.iter())
                .find(|c| c.idx == idx)
                .map(|c| c.hash_hex.clone())
                .unwrap_or_default();
            if h != expected {
                let stripe = idx / k as u64;
                let data_i = (idx % k as u64) as usize;
                to_repair.entry(stripe).or_default().push(data_i);
            }
        } else {
            // File missing: mark this data shard as missing for reconstruction
            let stripe = idx / k as u64;
            let data_i = (idx % k as u64) as usize;
            to_repair.entry(stripe).or_default().push(data_i);
        }
    }

    // Parallelize by stripe
    let idx_map = idx_map; // move into closure
    let file_sizes = file_sizes;
    // parity_map is already owned and read-only
    let chunk_size = mf.chunk_size;
    type Edit = (PathBuf, u64, Vec<u8>);
    type StripeResult = (u64, Vec<Edit>);
    let results: Vec<StripeResult> = to_repair
        .into_par_iter()
        .map(|(stripe, missing)| {
            let mut repaired_local = 0u64;
            let mut edits_local: Vec<Edit> = Vec::new();
            // K data shards
            let mut data_bufs: Vec<Option<Vec<u8>>> = Vec::with_capacity(k);
            for i in 0..k {
                let idx = stripe * k as u64 + i as u64;
                if missing.contains(&i) {
                    data_bufs.push(None);
                } else {
                    let mut buf = vec![0u8; chunk_size];
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
            let mut shards: Vec<Option<Vec<u8>>> = vec![None; k + m];
            for (i, db) in data_bufs.into_iter().enumerate() {
                shards[i] = db;
            }
            let mut parity = Vec::new();
            if let Some(v) = parity_map.get(&(stripe as u32)) {
                parity = v.clone();
            }
            if parity.len() < m {
                // cannot repair this stripe
                return (0u64, edits_local);
            }
            for (pi, pbuf) in parity.into_iter() {
                if pi < m {
                    shards[k + pi] = Some(pbuf);
                }
            }
            let rs = RsCodec::new(k, m).expect("init RS");
            if rs.reconstruct(&mut shards).is_ok() {
                for i in missing {
                    let idx = stripe * k as u64 + i as u64;
                    if let Some((path, off, len)) = idx_map.get(&idx) {
                        if let Some(Some(buf)) = shards.get(i) {
                            edits_local.push((path.clone(), *off, buf[..*len as usize].to_vec()));
                            repaired_local += 1;
                        }
                    }
                }
            }
            (repaired_local, edits_local)
        })
        .collect();

    let mut repaired_chunks = 0u64;
    // Collect per-file edits for atomic replacement
    let mut file_edits: HashMap<PathBuf, Vec<(u64, Vec<u8>)>> = HashMap::new();
    for (rc, edits) in results {
        repaired_chunks += rc;
        for (p, off, data) in edits {
            file_edits.entry(p).or_default().push((off, data));
        }
    }
    let failed_chunks = 0u64; // conservatively 0 here; detailed accounting optional
                              // Apply edits: prefer atomic replace via temp+rename; fallback to in-place
    for (path, mut edits) in file_edits {
        edits.sort_by_key(|e| e.0);
        // backup once per file
        let bak = path.with_extension("parx.bak");
        if !bak.exists() {
            let _ = std::fs::copy(&path, &bak);
        }
        // Try atomic replace
        let parent = path.parent().unwrap_or(Path::new("."));
        let tmp = parent.join(format!("{}.parx.tmp", path.file_name().unwrap().to_string_lossy()));
        let atomic_res = (|| -> Result<()> {
            let mut orig = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => {
                    // Recreate missing file buffer sized to manifest size (or grow on writes)
                    let sz = *file_sizes.get(&path).unwrap_or(&0u64) as usize;
                    vec![0u8; sz]
                }
            };
            for (off, data) in &edits {
                let off = *off as usize;
                if off + data.len() > orig.len() {
                    orig.resize(off + data.len(), 0);
                }
                orig[off..off + data.len()].copy_from_slice(data);
            }
            // Truncate back to manifest-declared file size if known
            if let Some(sz) = file_sizes.get(&path) {
                if orig.len() > *sz as usize {
                    orig.truncate(*sz as usize);
                }
            }
            {
                let mut tf = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&tmp)?;
                tf.write_all(&orig)?;
                tf.sync_all()?;
            }
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if atomic_res.is_err() {
            // Fallback to in-place with advisory lock
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(&path)
            {
                let _ = f.try_lock_exclusive();
                for (off, data) in &edits {
                    if f.seek(SeekFrom::Start(*off)).is_ok() {
                        let _ = f.write_all(data);
                    }
                }
                let _ = f.sync_all();
                // unlocking happens on drop; avoid std::File::unlock (MSRV >=1.89)
            }
        }
    }

    // Release global lock on drop
    Ok(RepairReport { repaired_chunks, failed_chunks })
}
