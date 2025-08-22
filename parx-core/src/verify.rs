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
    let per_file: Result<Vec<(u64, u64, Vec<blake3::Hash>)>> = mf
        .files
        .par_iter()
        .map(|fe| -> Result<(u64, u64, Vec<blake3::Hash>)> {
            let path = validate_path(root, Path::new(&fe.rel_path), policy)
                .with_context(|| format!("validate path {:?}", fe.rel_path))?;
            let mut f = File::open(&path).with_context(|| format!("open {:?}", path))?;
            let mut ok = 0u64;
            let mut bad = 0u64;
            let mut hashes = Vec::with_capacity(fe.chunks.len());
            for ch in &fe.chunks {
                let mut buf = vec![0u8; mf.chunk_size];
                f.seek(SeekFrom::Start(ch.file_offset))?;
                let mut small = vec![0u8; ch.len as usize];
                f.read_exact(&mut small)?;
                buf[..small.len()].copy_from_slice(&small);
                let h = blake3::hash(&buf);
                if h.to_hex().to_string() == ch.hash_hex {
                    ok += 1;
                } else {
                    bad += 1;
                }
                hashes.push(h);
            }
            Ok((ok, bad, hashes))
        })
        .collect();
    let per_file = per_file?;
    let mut chunks_ok = 0u64;
    let mut chunks_bad = 0u64;
    let mut all_hashes = Vec::new();
    for (ok, bad, hashes) in per_file {
        chunks_ok += ok;
        chunks_bad += bad;
        all_hashes.extend(hashes);
    }
    let merkle_ok = merkle::root(&all_hashes).to_hex().to_string() == mf.merkle_root_hex;
    Ok(VerifyReport { chunks_ok, chunks_bad, merkle_ok })
}
