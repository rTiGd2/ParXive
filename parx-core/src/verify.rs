use crate::manifest::Manifest;
use crate::merkle;
use crate::path_safety::{validate_path, PathPolicy};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub chunks_ok: u64,
    pub chunks_bad: u64,
    pub merkle_ok: bool,
}

pub fn verify(manifest_path: &Path, root: &Path) -> Result<VerifyReport> {
    verify_with_policy(manifest_path, root, PathPolicy::default())
}

pub fn verify_with_policy(
    manifest_path: &Path,
    root: &Path,
    policy: PathPolicy,
) -> Result<VerifyReport> {
    let mf: Manifest =
        serde_json::from_reader(File::open(manifest_path)?).context("read manifest.json")?;
    verify_with_manifest(mf, root, policy)
}

pub fn verify_with_manifest(mf: Manifest, root: &Path, policy: PathPolicy) -> Result<VerifyReport> {
    let per_file: Result<Vec<(u64, u64, Vec<(u64, blake3::Hash)>)>> = mf
        .files
        .par_iter()
        .map(|fe| -> Result<(u64, u64, Vec<(u64, blake3::Hash)>)> {
            let path = validate_path(root, Path::new(&fe.rel_path), policy)
                .with_context(|| format!("validate path {:?}", fe.rel_path))?;
            let mut f = File::open(&path).with_context(|| format!("open {:?}", path))?;
            let mut ok = 0u64;
            let mut bad = 0u64;
            let mut hashes = Vec::with_capacity(fe.chunks.len());
            // Reuse a single buffer for all chunks of this file
            let mut buf = vec![0u8; mf.chunk_size];
            for ch in &fe.chunks {
                f.seek(SeekFrom::Start(ch.file_offset))?;
                let want = ch.len as usize;
                // Read directly into the reusable buffer
                if want > 0 {
                    f.read_exact(&mut buf[..want])?;
                }
                // Zero any remaining bytes to keep deterministic hashing
                if want < mf.chunk_size {
                    for b in &mut buf[want..] {
                        *b = 0;
                    }
                }
                let h = blake3::hash(&buf);
                if h.to_hex().to_string() == ch.hash_hex {
                    ok += 1;
                } else {
                    bad += 1;
                }
                hashes.push((ch.idx, h));
            }
            Ok((ok, bad, hashes))
        })
        .collect();
    let per_file = per_file?;
    let mut chunks_ok = 0u64;
    let mut chunks_bad = 0u64;
    // Reconstruct global order by idx across all files
    let mut ordered: Vec<Option<blake3::Hash>> = vec![None; mf.total_chunks as usize];
    for (ok, bad, hashes) in per_file {
        chunks_ok += ok;
        chunks_bad += bad;
        for (idx, h) in hashes {
            ordered[idx as usize] = Some(h);
        }
    }
    // Ensure all positions are filled
    let mut all_hashes: Vec<blake3::Hash> = Vec::with_capacity(ordered.len());
    for o in ordered.into_iter() {
        all_hashes.push(o.expect("missing chunk hash while reconstructing global order"));
    }
    let merkle_ok = merkle::root(&all_hashes).to_hex().to_string() == mf.merkle_root_hex;
    Ok(VerifyReport { chunks_ok, chunks_bad, merkle_ok })
}
