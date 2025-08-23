use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::compute::{ComputeBackend, CpuBackend};
use crate::manifest::{ChunkRef, FileEntry, Manifest};
use crate::merkle;
use crate::volume::{vol_name, VolumeEntry};
use fs2::FileExt;

pub struct EncoderConfig {
    pub chunk_size: usize,
    pub stripe_k: usize,
    pub parity_pct: u32,
    pub volumes: usize,
    pub outer_group: usize,
    pub outer_parity: usize,
    pub interleave_files: bool,
}

pub struct Encoder;

impl Encoder {
    pub fn encode(root: &Path, output: &Path, cfg: &EncoderConfig) -> Result<Manifest> {
        // Validate configuration early
        if cfg.chunk_size == 0 {
            anyhow::bail!("chunk_size must be > 0");
        }
        if cfg.stripe_k == 0 {
            anyhow::bail!("stripe_k must be > 0");
        }
        if cfg.parity_pct > 100 {
            anyhow::bail!("parity_pct must be <= 100");
        }
        if cfg.volumes == 0 || cfg.volumes > 256 {
            anyhow::bail!("volumes must be in 1..=256");
        }

        // 1) Discover files (regular files only, skip .parx)
        let mut files: Vec<PathBuf> = Vec::new();
        for ent in walkdir::WalkDir::new(root).min_depth(1) {
            let ent = ent?;
            let p = ent.path();
            if ent.file_type().is_dir() {
                continue;
            }
            if !ent.file_type().is_file() {
                continue;
            }
            if p.components().any(|c| c.as_os_str() == ".parx") {
                continue;
            }
            files.push(p.to_path_buf());
        }
        files.sort();

        // 2) Chunk layout (collect per-file first, assign global order later)
        struct TmpChunk {
            len: u32,
            file_offset: u64,
            hash: blake3::Hash,
        }
        struct TmpFile {
            rel_path: String,
            size: u64,
            chunks: Vec<TmpChunk>,
        }

        let mut tmp_files: Vec<TmpFile> = Vec::new();
        let mut total_bytes: u64 = 0;
        for path in &files {
            // Prefer a simple prefix strip since WalkDir yields paths under `root`.
            // This avoids macOS `/var` -> `/private/var` symlink quirks and ensures
            // manifest relpaths never contain parent traversal segments.
            let mut rel_opt = path.strip_prefix(root).ok().map(|p| p.to_path_buf());
            if rel_opt.is_none() {
                if let (Ok(root_can), Ok(path_can)) = (root.canonicalize(), path.canonicalize()) {
                    if let Ok(p) = path_can.strip_prefix(&root_can) {
                        rel_opt = Some(p.to_path_buf());
                    }
                }
            }
            let rel = rel_opt.with_context(|| format!("walked path not under root: {:?}", path))?;
            let rel_path = rel.to_string_lossy().to_string();
            let mut f = File::open(path).with_context(|| format!("open {:?}", path))?;
            let size = f.metadata()?.len();
            total_bytes += size;
            let mut remaining = size;
            let mut file_offset = 0u64;
            let mut chunks = Vec::new();
            while remaining > 0 {
                let to_read = std::cmp::min(remaining, cfg.chunk_size as u64) as usize;
                let mut buf = vec![0u8; cfg.chunk_size];
                let mut filled = 0usize;
                while filled < to_read {
                    let n = f.read(&mut buf[filled..to_read])?;
                    if n == 0 {
                        break;
                    }
                    filled += n;
                }
                if filled == 0 {
                    break;
                }
                if filled < cfg.chunk_size {
                    for b in &mut buf[filled..] {
                        *b = 0;
                    }
                }
                let hash = blake3::hash(&buf);
                chunks.push(TmpChunk { len: filled as u32, file_offset, hash });
                remaining -= filled as u64;
                file_offset += filled as u64;
            }
            tmp_files.push(TmpFile { rel_path, size, chunks });
        }

        // Assign global ordering: sequential per file or round-robin across files
        let mut order: Vec<(usize, usize)> = Vec::new(); // (file_idx, local_chunk_idx)
        if cfg.interleave_files {
            let mut rr = 0usize;
            loop {
                let mut appended = false;
                for (fi, tf) in tmp_files.iter().enumerate() {
                    if rr < tf.chunks.len() {
                        order.push((fi, rr));
                        appended = true;
                    }
                }
                if !appended {
                    break;
                }
                rr += 1;
            }
        } else {
            for (fi, tf) in tmp_files.iter().enumerate() {
                for ci in 0..tf.chunks.len() {
                    order.push((fi, ci));
                }
            }
        }

        // Build manifest file entries with global idx, and Merkle list
        let mut all_chunk_hashes = Vec::with_capacity(order.len());
        let mut map_global_to_local: Vec<(usize, usize)> = Vec::with_capacity(order.len());
        let mut file_entries: Vec<FileEntry> = tmp_files
            .iter()
            .map(|tf| FileEntry {
                rel_path: tf.rel_path.clone(),
                size: tf.size,
                chunks: Vec::new(),
            })
            .collect();
        let mut next_idx: u64 = 0;
        for (fi, ci) in order {
            let tc = &tmp_files[fi].chunks[ci];
            all_chunk_hashes.push(tc.hash);
            map_global_to_local.push((fi, ci));
            file_entries[fi].chunks.push(ChunkRef {
                idx: next_idx,
                file_offset: tc.file_offset,
                len: tc.len,
                hash_hex: tc.hash.to_hex().to_string(),
            });
            next_idx += 1;
        }

        // 4) Compute RS parity per stripe and write volumes (round-robin placement)
        std::fs::create_dir_all(output).with_context(|| format!("create dir {:?}", output))?;
        let vol_count = cfg.volumes.max(1);

        // Open volumes, write placeholder headers
        #[derive(Debug)]
        struct VolState(File, u64, Vec<VolumeEntry>); // (file, current_offset, index)
        let mut files_out: Vec<VolState> = Vec::new();
        for vid in 0..vol_count {
            let path = output.join(vol_name(vid));
            let f = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .with_context(|| format!("create {:?}", path))?;
            // Take an exclusive OS-level lock for the duration of encode
            f.lock_exclusive().context("lock volume file")?;
            // placeholder header (entries=0 for now)
            super_write_simple_header(&f, cfg.stripe_k as u32, 0, 0)?;
            files_out.push(VolState(f, 0, Vec::new()));
        }

        // Inner RS
        let k = cfg.stripe_k;
        let mut m = (cfg.stripe_k as u64 * cfg.parity_pct as u64).div_ceil(100);
        if cfg.parity_pct == 0 {
            m = 0;
        }
        let m = m as usize;
        if k + m == 0 || k + m > 256 {
            anyhow::bail!("invalid RS parameters: k+m must be in 1..=256 (k={}, m={})", k, m);
        }

        if m > 0 {
            use rayon::prelude::*;
            use std::sync::{Arc, Mutex};
            let total_chunks = map_global_to_local.len();
            let stripes = total_chunks.div_ceil(k);
            // Wrap volumes for synchronized concurrent appends
            let vols: Vec<_> =
                files_out.into_iter().map(|state| Arc::new(Mutex::new(state))).collect();
            let root_path = root.to_path_buf();
            let tmp_files_ref = &tmp_files;
            let map_ref = &map_global_to_local;
            let backend = CpuBackend::new(k, m)?;
            (0..stripes).into_par_iter().try_for_each(|s| -> Result<()> {
                // Build data shards for this stripe
                let mut data_bufs: Vec<Vec<u8>> =
                    (0..k).map(|_| vec![0u8; cfg.chunk_size]).collect();
                let mut stripe_len: usize = 0; // actual bytes in this stripe (<= chunk_size)
                                               // Cache file handles within this stripe to avoid reopen overhead
                let mut file_cache: std::collections::HashMap<std::path::PathBuf, File> =
                    std::collections::HashMap::new();
                for i in 0..k {
                    let idx = s * k + i;
                    if idx < total_chunks {
                        let (fi, ci) = map_ref[idx];
                        let tf = &tmp_files_ref[fi];
                        let tc = &tf.chunks[ci];
                        let path = root_path.join(&tf.rel_path);
                        let f = match file_cache.get_mut(&path) {
                            Some(f) => f,
                            None => {
                                let f = File::open(&path)
                                    .with_context(|| format!("open {:?}", path))?;
                                file_cache.insert(path.clone(), f);
                                file_cache.get_mut(&path).unwrap()
                            }
                        };
                        let buf = &mut data_bufs[i];
                        f.seek(SeekFrom::Start(tc.file_offset)).context("seek chunk")?;
                        let to_read = tc.len as usize;
                        if to_read > 0 {
                            f.read_exact(&mut buf[..to_read]).context("read chunk")?;
                        }
                        if to_read < cfg.chunk_size {
                            for b in &mut buf[to_read..] {
                                *b = 0;
                            }
                        }
                        if to_read > stripe_len {
                            stripe_len = to_read;
                        }
                    } else {
                        // already zeroed buffers
                    }
                }
                let mut parity_bufs: Vec<Vec<u8>> =
                    (0..m).map(|_| vec![0u8; cfg.chunk_size]).collect();
                let data_refs: Vec<&[u8]> = data_bufs.iter().map(|v| &v[..]).collect();
                let mut parity_refs: Vec<&mut [u8]> =
                    parity_bufs.iter_mut().map(|v| v.as_mut_slice()).collect();
                backend.encode_stripe(&data_refs[..], &mut parity_refs[..])?;
                // Append parity shards to volumes, trimming to actual stripe_len to avoid padding
                for (pi, pbuf) in parity_bufs.into_iter().enumerate() {
                    let vid = pi % vol_count;
                    let mut guard =
                        vols[vid].lock().map_err(|e| anyhow::anyhow!("poisoned lock: {e}"))?;
                    let VolState(ref mut vf, ref mut current_offset, ref mut vindex) = *guard;
                    let off = *current_offset;
                    vf.seek(SeekFrom::Start(off)).context("seek start")?;
                    let write_len = if stripe_len == 0 { 0 } else { stripe_len };
                    vf.write_all(&pbuf[..write_len]).context("write parity")?;
                    *current_offset += write_len as u64;
                    vindex.push(VolumeEntry {
                        stripe: s as u32,
                        parity_idx: pi as u16,
                        offset: off,
                        len: write_len as u32,
                        hash: None,
                        outer_for_stripe: None,
                    });
                }
                Ok(())
            })?;
            // Unwrap volumes back
            let mut files_out_unwrapped: Vec<VolState> = Vec::new();
            for v in vols {
                let pair = Arc::try_unwrap(v).expect("unwrap arc").into_inner().expect("unlock");
                files_out_unwrapped.push(pair);
            }
            files_out = files_out_unwrapped;
        }

        // Fill manifest chunk hashes from computed vector
        for (gidx, (fi, ci)) in map_global_to_local.iter().enumerate() {
            file_entries[*fi].chunks[*ci].hash_hex = all_chunk_hashes[gidx].to_hex().to_string();
        }

        // Finalize: write manifest backup (vol-000 only), then indices and headers
        // Serialize manifest once for backup payload
        let manifest_preview = Manifest {
            created_utc: chrono::Utc::now().to_rfc3339(),
            chunk_size: cfg.chunk_size,
            stripe_k: cfg.stripe_k,
            parity_pct: cfg.parity_pct,
            total_bytes,
            total_chunks: next_idx,
            files: file_entries.clone(),
            merkle_root_hex: merkle::root(&all_chunk_hashes).to_hex().to_string(),
            parity_dir: output.to_string_lossy().to_string(),
            volumes: vol_count,
            outer_group: cfg.outer_group,
            outer_parity: cfg.outer_parity,
        };
        let manifest_json = serde_json::to_vec_pretty(&manifest_preview)?;

        // If we can, prepare manifest-backup metadata for vol-000 trailer TLV
        let mut mb_meta: Option<crate::index::ManifestBackupMeta> = None;
        if let Some(VolState(vf0, _, _)) = files_out.get_mut(0) {
            // Write backup payload to vol-000 and capture its location
            let compressed = zstd::stream::encode_all(&manifest_json[..], 0)?;
            let mb_off = vf0.metadata()?.len();
            let mb_len = compressed.len() as u32;
            let mut h = crc32fast::Hasher::new();
            h.update(&compressed);
            let mb_crc = h.finalize();
            vf0.seek(SeekFrom::End(0))?;
            vf0.write_all(&compressed)?;
            mb_meta =
                Some(crate::index::ManifestBackupMeta { off: mb_off, len: mb_len, crc32: mb_crc });
        }

        for (vid, VolState(vf, _off, vindex)) in files_out.iter_mut().enumerate() {
            let meta = if vid == 0 { mb_meta } else { None };
            crate::index::write_index_and_trailer(vf, vindex, meta)?;
            super_write_simple_header(vf, k as u32, m as u32, vindex.len() as u32)?;
        }

        // Manifest
        let manifest = manifest_preview;
        let mpath = output.join("manifest.json");
        let mut mf = File::create(&mpath).context("create manifest.json")?;
        mf.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

        Ok(manifest)
    }
}

// Simple header writer (keeps CLI/header semantics consistent)
fn super_write_simple_header(mut f: &File, k: u32, m: u32, entries: u32) -> Result<()> {
    // Reuse reserved 12 bytes to store versioning/flags while keeping total size constant
    // Layout: magic(8) + k(4) + m(4) + entries(4) + version(4) + header_len(4) + feature_flags(4)
    let mut buf = Vec::with_capacity(8 + 4 + 4 + 4 + 12);
    buf.extend_from_slice(b"PARXVOL\0");
    buf.extend_from_slice(&k.to_le_bytes());
    buf.extend_from_slice(&m.to_le_bytes());
    buf.extend_from_slice(&entries.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes()); // version=1
    let header_len: u32 = 8 + 4 + 4 + 4 + 12; // 32 bytes
    buf.extend_from_slice(&header_len.to_le_bytes()); // header_len
    buf.extend_from_slice(&0u32.to_le_bytes()); // feature_flags
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&buf)?;
    Ok(())
}
