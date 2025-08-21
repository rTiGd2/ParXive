use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::manifest::{ChunkRef, FileEntry, Manifest};
use crate::merkle;
use crate::rs_codec::RsCodec;
use crate::volume::{vol_name, VolumeEntry};

pub struct EncoderConfig {
    pub chunk_size: usize,
    pub stripe_k: usize,
    pub parity_pct: u32,
    pub volumes: usize,
    pub outer_group: usize,
    pub outer_parity: usize,
}

pub struct Encoder;

impl Encoder {
    pub fn encode(root: &Path, output: &Path, cfg: &EncoderConfig) -> Result<Manifest> {
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

        // 2) Chunk and hash
        let mut file_entries = Vec::new();
        let mut all_chunk_hashes = Vec::new();
        let mut chunk_buffers: Vec<Vec<u8>> = Vec::new();
        let mut total_bytes: u64 = 0;
        let mut next_chunk_idx: u64 = 0;
        for path in files {
            let rel_path = pathdiff::diff_paths(&path, root)
                .unwrap_or_else(|| path.file_name().unwrap().into());
            let rel_path = rel_path.to_string_lossy().to_string();
            let mut f = File::open(&path).with_context(|| format!("open {:?}", path))?;
            let size = f.metadata()?.len();
            total_bytes += size;
            let mut remaining = size;
            let mut file_offset = 0u64;
            let mut chunks = Vec::new();
            while remaining > 0 {
                let to_read = std::cmp::min(remaining, cfg.chunk_size as u64) as usize;
                let mut buf = vec![0u8; cfg.chunk_size];
                let readn = f.read(&mut buf[..to_read])?;
                if readn == 0 {
                    break;
                }
                if readn < cfg.chunk_size {
                    for b in &mut buf[readn..] {
                        *b = 0;
                    }
                }
                let h = blake3::hash(&buf);
                let hash_hex = h.to_hex().to_string();
                all_chunk_hashes.push(h);
                chunks.push(ChunkRef {
                    idx: next_chunk_idx,
                    file_offset,
                    len: readn as u32,
                    hash_hex,
                });
                chunk_buffers.push(buf);
                next_chunk_idx += 1;
                remaining -= readn as u64;
                file_offset += readn as u64;
            }
            file_entries.push(FileEntry { rel_path, size, chunks });
        }

        // 3) Merkle root
        let merkle_root_hex = merkle::root(&all_chunk_hashes).to_hex().to_string();

        // 4) Compute RS parity per stripe and write volumes (round-robin placement)
        std::fs::create_dir_all(output).with_context(|| format!("create dir {:?}", output))?;
        let vol_count = cfg.volumes.max(1);

        // Open volumes, write placeholder headers
        let mut files_out: Vec<(File, Vec<VolumeEntry>)> = Vec::new();
        for vid in 0..vol_count {
            let path = output.join(vol_name(vid));
            let f = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .with_context(|| format!("create {:?}", path))?;
            // placeholder header (entries=0 for now)
            super_write_simple_header(&f, cfg.stripe_k as u32, 0, 0)?;
            files_out.push((f, Vec::new()));
        }

        // Inner RS
        let k = cfg.stripe_k;
        let mut m = (cfg.stripe_k as u64 * cfg.parity_pct as u64).div_ceil(100);
        if cfg.parity_pct == 0 {
            m = 0;
        }
        let m = m as usize;
        if m > 0 {
            let rs = RsCodec::new(k, m).context("init RS")?;
            let total_chunks = chunk_buffers.len();
            let stripes = total_chunks.div_ceil(k);
            for s in 0..stripes {
                let mut data_bufs: Vec<Vec<u8>> = Vec::with_capacity(k);
                for i in 0..k {
                    let idx = s * k + i;
                    if idx < total_chunks {
                        data_bufs.push(chunk_buffers[idx].clone());
                    } else {
                        data_bufs.push(vec![0u8; cfg.chunk_size]);
                    }
                }
                let mut parity_bufs: Vec<Vec<u8>> =
                    (0..m).map(|_| vec![0u8; cfg.chunk_size]).collect();
                let mut shards: Vec<&mut [u8]> = Vec::with_capacity(k + m);
                for b in &mut data_bufs {
                    shards.push(b.as_mut_slice());
                }
                for b in &mut parity_bufs {
                    shards.push(b.as_mut_slice());
                }
                rs.encode(&mut shards[..]).context("RS encode")?;
                // Place parity shards and record entries
                for (pi, pbuf) in parity_bufs.into_iter().enumerate() {
                    let vid = pi % vol_count;
                    let (ref mut vf, ref mut vindex) = files_out[vid];
                    let off = vf.metadata()?.len();
                    vf.seek(SeekFrom::End(0))?;
                    vf.write_all(&pbuf)?;
                    vindex.push(VolumeEntry {
                        stripe: s as u32,
                        parity_idx: pi as u16,
                        offset: off,
                        len: cfg.chunk_size as u32,
                        hash: None,
                        outer_for_stripe: None,
                    });
                }
            }
        }

        // Finalize indices and headers
        for (vf, vindex) in files_out.iter_mut() {
            crate::index::write_index_and_trailer(vf, vindex)?;
            super_write_simple_header(vf, k as u32, m as u32, vindex.len() as u32)?;
        }

        // Manifest
        let manifest = Manifest {
            created_utc: chrono::Utc::now().to_rfc3339(),
            chunk_size: cfg.chunk_size,
            stripe_k: cfg.stripe_k,
            parity_pct: cfg.parity_pct,
            total_bytes,
            total_chunks: next_chunk_idx,
            files: file_entries,
            merkle_root_hex,
            parity_dir: output.to_string_lossy().to_string(),
            volumes: vol_count,
            outer_group: cfg.outer_group,
            outer_parity: cfg.outer_parity,
        };
        let mpath = output.join("manifest.json");
        let mut mf = File::create(&mpath).context("create manifest.json")?;
        mf.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

        Ok(manifest)
    }
}

// Simple header writer (keeps CLI/header semantics consistent)
fn super_write_simple_header(mut f: &File, k: u32, m: u32, entries: u32) -> Result<()> {
    let mut buf = Vec::with_capacity(8 + 4 + 4 + 4 + 12);
    buf.extend_from_slice(b"PARXVOL\0");
    buf.extend_from_slice(&k.to_le_bytes());
    buf.extend_from_slice(&m.to_le_bytes());
    buf.extend_from_slice(&entries.to_le_bytes());
    buf.extend_from_slice(&[0u8; 12]);
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&buf)?;
    Ok(())
}
