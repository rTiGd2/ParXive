
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSetBuilder};
use memmap2::Mmap;
use parx_core::cuda_backend::cuda::CudaCtx;
use parx_core::manifest::{ChunkRef, FileEntry, Manifest};
use parx_core::progress::Progress;
use parx_core::rs_codec::RsCodec;
use parx_core::volume::{vol_name, VolumeEntry, VolumeHeaderBin};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_CHUNK: usize = 1 << 20;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum GpuMode {
    Auto,
    On,
    Off,
}

#[derive(Parser)]
#[command(name = "parx", version, about = "parx v0.6.0")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create parity set
    Create {
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=120))]
        parity: u32,
        #[arg(long, default_value_t = 64)]
        stripe_k: usize,
        #[arg(long, default_value_t = DEFAULT_CHUNK)]
        chunk_size: usize,
        #[arg(long, default_value = ".parx")]
        output: PathBuf,
        /// Comma-separated sizes per volume (e.g., 32M,32M,32M)
        #[arg(long)]
        volume_sizes: Option<String>,
        /// Comma-separated block counts per volume (advanced)
        #[arg(long)]
        volume_counts: Option<String>,
        #[arg(long, default_value_t = 256)]
        outer_group: usize,
        #[arg(long, default_value_t = 2)]
        outer_parity: usize,
        #[arg(long)]
        include: Vec<String>,
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long, default_value_t = false)]
        progress: bool,
        #[arg(long, value_enum, default_value_t = GpuMode::Auto)]
        gpu: GpuMode,
        inputs: Vec<PathBuf>,
    },
    /// Quick header/index check of volumes
    Quickcheck { parx_dir: PathBuf },
    /// Verify all source files against manifest
    Verify { manifest: PathBuf, root: PathBuf },
    /// Audit missing/corrupt source chunks by stripe
    Audit { manifest: PathBuf, root: PathBuf },
    /// Attempt repair of missing/corrupt source chunks
    Repair { manifest: PathBuf, root: PathBuf },
    /// Parity-aware audit of volume health (counts + optional hash verify)
    Paritycheck { parx_dir: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Create {
            parity,
            stripe_k,
            chunk_size,
            output,
            volume_sizes,
            volume_counts,
            outer_group,
            outer_parity,
            include,
            exclude,
            progress,
            gpu,
            inputs,
        } => {
            create(
                parity,
                stripe_k,
                chunk_size,
                &output,
                volume_sizes,
                volume_counts,
                outer_group,
                outer_parity,
                &include,
                &exclude,
                progress,
                gpu,
                &inputs,
            )?;
        }
        Cmd::Quickcheck { parx_dir } => quickcheck(&parx_dir)?,
        Cmd::Verify { manifest, root } => verify(&manifest, &root)?,
        Cmd::Audit { manifest, root } => audit(&manifest, &root)?,
        Cmd::Repair { manifest, root } => repair(&manifest, &root)?,
        Cmd::Paritycheck { parx_dir } => paritycheck(&parx_dir)?,
    }
    Ok(())
}

fn build_globset(
    includes: &[String],
    excludes: &[String],
) -> Result<(globset::GlobSet, globset::GlobSet)> {
    let mut incb = GlobSetBuilder::new();
    let mut excb = GlobSetBuilder::new();
    if includes.is_empty() {
        incb.add(Glob::new("**/*")?);
    }
    for g in includes {
        incb.add(Glob::new(g)?);
    }
    for g in excludes {
        excb.add(Glob::new(g)?);
    }
    Ok((incb.build()?, excb.build()?))
}

fn list_files(
    inputs: &[PathBuf],
    inc: &globset::GlobSet,
    exc: &globset::GlobSet,
) -> Result<Vec<PathBuf>> {
    let mut v = vec![];
    for p in inputs {
        let md = fs::metadata(p).with_context(|| format!("stat {}", p.display()))?;
        if md.is_dir() {
            for e in WalkDir::new(p).into_iter().filter_map(|e| e.ok()) {
                let path = e.path();
                if !e.file_type().is_file() {
                    continue;
                }
                let rp =
                    pathdiff::diff_paths(path, std::env::current_dir()?).unwrap_or_else(|| {
                        path.to_path_buf()
                    });
                let rp_str = rp.to_string_lossy().replace('\\', "/");
                if !inc.is_match(&rp_str) {
                    continue;
                }
                if !exc.is_match(&rp_str) {
                    v.push(path.to_path_buf());
                }
            }
        } else if md.is_file() {
            v.push(p.clone());
        }
    }
    v.sort();
    Ok(v)
}

fn parse_sizes(spec: &str, block: usize) -> Result<Vec<usize>> {
    let mut out = vec![];
    for part in spec.split(',') {
        let s = part.trim().to_uppercase();
        let (num, mul) = if s.ends_with('K') {
            (&s[..s.len() - 1], 1 << 10)
        } else if s.ends_with('M') {
            (&s[..s.len() - 1], 1 << 20)
        } else if s.ends_with('G') {
            (&s[..s.len() - 1], 1 << 30)
        } else {
            (&s[..], 1)
        };
        let v: usize = num.parse().map_err(|_| anyhow!("bad size {}", part))?;
        let blocks = (v * mul).div_ceil(block);
        out.push(blocks.max(1));
    }
    Ok(out)
}
fn parse_counts(spec: &str) -> Result<Vec<usize>> {
    let mut out = vec![];
    for part in spec.split(',') {
        let v: usize = part.trim().parse().map_err(|_| anyhow!("bad count {}", part))?;
        out.push(v.max(1));
    }
    Ok(out)
}

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s_since_epoch", now.as_secs())
}

fn merkle_root_blake3(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut cur: Vec<[u8; 32]> = leaves.to_vec();
    while cur.len() > 1 {
        let mut next: Vec<[u8; 32]> = Vec::with_capacity(cur.len().div_ceil(2));
        for pair in cur.chunks(2) {
            if pair.len() == 2 {
                let mut h = blake3::Hasher::new();
                h.update(&pair[0]);
                h.update(&pair[1]);
                next.push(*h.finalize().as_bytes());
            } else {
                next.push(pair[0]);
            }
        }
        cur = next;
    }
    cur[0]
}

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0xF) as usize] as char);
    }
    s
}

#[allow(clippy::too_many_arguments)]
fn create(
    parity_pct: u32,
    stripe_k: usize,
    chunk_size: usize,
    out_dir: &Path,
    vol_sizes: Option<String>,
    vol_counts: Option<String>,
    _outer_group: usize,
    outer_parity: usize,
    includes: &[String],
    excludes: &[String],
    show_progress: bool,
    gpu: GpuMode,
    inputs: &[PathBuf],
) -> Result<()> {
    fs::create_dir_all(out_dir)?;
    let (inc, exc) = build_globset(includes, excludes)?;
    let files_sorted = list_files(inputs, &inc, &exc)?;

    #[derive(Clone)]
    struct FInfo {
        path: PathBuf,
        size: u64,
        base_idx: u64,
        chunks: u64,
    }
    let finfos: Vec<FInfo> = {
        let mut infos = vec![];
        let mut base = 0u64;
        for p in &files_sorted {
            let sz = fs::metadata(p)?.len();
            let chunks = sz.div_ceil(chunk_size as u64);
            infos.push(FInfo {
                path: p.clone(),
                size: sz,
                base_idx: base,
                chunks,
            });
            base += chunks;
        }
        infos
    };

    let prog = Progress::new(show_progress);
    prog.set_stage("Hashing");
    prog.start();

    let hashed: Vec<(usize, FileEntry, Vec<[u8; 32]>)> = finfos
        .par_iter()
        .enumerate()
        .map(|(fi, info)| -> Result<(usize, FileEntry, Vec<[u8; 32]>)> {
            let rel = make_rel_path(&info.path)?;
            let mut reader = std::io::BufReader::new(File::open(&info.path)?);
            let mut chunks = Vec::with_capacity(info.chunks as usize);
            let mut chunk_hashes = Vec::with_capacity(info.chunks as usize);
            let mut offset: u64 = 0;
            let mut global = info.base_idx;
            let mut buf = vec![0u8; chunk_size];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let dig = blake3::hash(&buf[..n]);
                chunk_hashes.push(*dig.as_bytes());
                chunks.push(ChunkRef {
                    idx: global,
                    file_offset: offset,
                    len: n as u32,
                    hash_hex: hex(dig.as_bytes()),
                });
                global += 1;
                offset += n as u64;
            }
            Ok((
                fi,
                FileEntry {
                    rel_path: rel,
                    size: info.size,
                    chunks,
                },
                chunk_hashes,
            ))
        })
        .collect::<Result<_>>()?;

    prog.stop();

    // Reassemble in input order
    let mut files: Vec<FileEntry> = vec![
        FileEntry { rel_path: String::new(), size: 0, chunks: vec![] };
        hashed.len()
    ];
    let total_chunks: usize = finfos.iter().map(|i| i.chunks as usize).sum();
    let mut chunk_hashes: Vec<[u8; 32]> = vec![[0; 32]; total_chunks];
    for (fi, fe, chs) in hashed {
        files[fi] = fe;
        let base = finfos[fi].base_idx as usize;
        for (i, h) in chs.into_iter().enumerate() {
            chunk_hashes[base + i] = h;
        }
    }

    let merkle_root = merkle_root_blake3(&chunk_hashes);
    let stripes = total_chunks.div_ceil(stripe_k);
    let m_per_stripe = ((parity_pct as f64 / 100.0) * (stripe_k as f64))
        .round()
        .max(1.0) as usize;

    eprintln!(
        "Parity: K={} M={} ({}%), stripes={} (outer M={})",
        stripe_k, m_per_stripe, parity_pct, stripes, outer_parity
    );

    // Volume allocation
    let counts: Vec<usize> = if let Some(spec) = vol_counts {
        parse_counts(&spec)?
    } else if let Some(spec) = vol_sizes {
        parse_sizes(&spec, chunk_size)?
    } else {
        vec![stripes * (m_per_stripe + outer_parity)]
    };
    let volumes = counts.len();

    // Manifest
    let mani = Manifest {
        created_utc: now_iso8601(),
        chunk_size,
        stripe_k,
        parity_pct,
        total_bytes: finfos.iter().map(|x| x.size).sum(),
        total_chunks: total_chunks as u64,
        files: files.clone(),
        merkle_root_hex: hex(&merkle_root),
        parity_dir: out_dir.to_string_lossy().to_string(),
        volumes,
        outer_group: m_per_stripe, // for now, per-stripe grouping
        outer_parity,
    };
    let manifest_path = out_dir.join("manifest.json");
    serde_json::to_writer_pretty(File::create(&manifest_path)?, &mani)?;
    let mani_hash = blake3::hash(&serde_json::to_vec(&mani)?);

    // Open volumes (PARXBV2)
    let mut vol_files: Vec<File> = vec![];
    let mut vol_offsets: Vec<u64> = vec![];
    let mut vol_entries: Vec<Vec<VolumeEntry>> = vec![vec![]; volumes];
    let mut hdr_lens: Vec<u32> = vec![];
    for i in 0..volumes {
        let vp = out_dir.join(vol_name(i));
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&vp)?;
        let header = VolumeHeaderBin {
            k: stripe_k as u32,
            m: m_per_stripe as u32,
            chunk_size: chunk_size as u32,
            total_chunks: total_chunks as u64,
            volume_id: i as u32,
            entries_len: 0, // filled later
            manifest_hash: *mani_hash.as_bytes(),
        };
        let header_bytes = bincode::serialize(&header)?;
        f.write_all(b"PARXBV2")?;
        let hdr_len_u32 = u32::try_from(header_bytes.len())?;
        f.write_all(&hdr_len_u32.to_le_bytes())?;
        f.write_all(&header_bytes)?;
        f.write_all(&0u32.to_le_bytes())?; // inline index placeholder
        hdr_lens.push(hdr_len_u32);
        vol_offsets.push(f.stream_position()?);
        vol_files.push(f);
    }

    // Optional CUDA sanity (no-op path for now)
    if let GpuMode::On = gpu {
        let _ = CudaCtx::new().and_then(|c| c.encode_noop());
    }

    // Stream source again & encode per stripe
    let mut readers: Vec<(PathBuf, File)> = files_sorted
        .iter()
        .map(|p| (p.clone(), File::open(p).unwrap()))
        .collect();
    let mut cur_file = 0usize;
    let mut buf = vec![0u8; chunk_size];

    // round-robin across volumes honoring remaining counts
    let mut vol_remaining = counts.clone();
    let mut next_vol = 0usize;

    let prog2 = Progress::new(true);
    prog2.set_stage("Encoding");
    prog2.set_blocks_total(stripes);
    prog2.start();

    for stripe in 0..stripes {
        let start = stripe * stripe_k;
        let end = ((stripe + 1) * stripe_k).min(total_chunks);
        let k_active = end - start;

        let mut shards: Vec<Vec<u8>> =
            (0..(k_active + m_per_stripe)).map(|_| vec![0u8; chunk_size]).collect();

        // fill data
        for dst in shards.iter_mut().take(k_active) {
            let n = read_next_chunk(&mut readers[..], &mut cur_file, &mut buf)?;
            dst[..n].copy_from_slice(&buf[..n]);
            if n < chunk_size {
                dst[n..].fill(0);
            }
        }

        // encode inner parity
        let rs = RsCodec::new(k_active, m_per_stripe)?;
        let mut refs: Vec<&mut [u8]> = shards.iter_mut().map(|v| v.as_mut_slice()).collect();
        rs.encode(&mut refs)?;

        // write inner parity with round-robin placement + hash per shard
        for pi in 0..m_per_stripe {
            // find next volume with capacity
            let mut tries = 0usize;
            while vol_remaining[next_vol] == 0 && tries < volumes {
                next_vol = (next_vol + 1) % volumes;
                tries += 1;
            }
            if vol_remaining[next_vol] == 0 {
                next_vol = 0;
            }
            let vi = next_vol;
            next_vol = (next_vol + 1) % volumes;

            let vf = &mut vol_files[vi];
            let off = vol_offsets[vi];
            let bytes = &refs[k_active + pi];
            vf.write_all(bytes)?;
            vol_offsets[vi] += bytes.len() as u64;
            vol_remaining[vi] = vol_remaining[vi].saturating_sub(1);

            let h = *blake3::hash(bytes).as_bytes();
            vol_entries[vi].push(VolumeEntry {
                stripe: stripe as u32,
                parity_idx: pi as u16,
                offset: off,
                len: chunk_size as u32,
                hash: Some(h),
                outer_for_stripe: None,
            });
        }

        // Outer parity-of-parity per stripe (if requested)
        if outer_parity > 0 {
            // Build data vector = inner parity shards (k = m_per_stripe)
            let mut data_and_par: Vec<Vec<u8>> =
                (0..(m_per_stripe + outer_parity)).map(|_| vec![0u8; chunk_size]).collect();
            // copy inner parity into first m slots
            for i in 0..m_per_stripe {
                data_and_par[i].copy_from_slice(&refs[k_active + i]);
            }
            // RS over parity
            let rs_outer = RsCodec::new(m_per_stripe, outer_parity)?;
            let mut refs_outer: Vec<&mut [u8]> =
                data_and_par.iter_mut().map(|v| v.as_mut_slice()).collect();
            rs_outer.encode(&mut refs_outer)?;

            // Write outer shards (index with stripe = u32::MAX, and outer_for_stripe=Some(stripe))
            for oi in 0..outer_parity {
                // find next volume with capacity
                let mut tries = 0usize;
                while vol_remaining[next_vol] == 0 && tries < volumes {
                    next_vol = (next_vol + 1) % volumes;
                    tries += 1;
                }
                if vol_remaining[next_vol] == 0 {
                    next_vol = 0;
                }
                let vi = next_vol;
                next_vol = (next_vol + 1) % volumes;

                let vf = &mut vol_files[vi];
                let off = vol_offsets[vi];
                let bytes = &refs_outer[m_per_stripe + oi];
                vf.write_all(bytes)?;
                vol_offsets[vi] += bytes.len() as u64;
                vol_remaining[vi] = vol_remaining[vi].saturating_sub(1);

                let h = *blake3::hash(bytes).as_bytes();
                vol_entries[vi].push(VolumeEntry {
                    stripe: u32::MAX,
                    parity_idx: oi as u16,
                    offset: off,
                    len: chunk_size as u32,
                    hash: Some(h),
                    outer_for_stripe: Some(stripe as u32),
                });
            }
        }

        if stripe % 128 == 0 {
            eprintln!(
                "  stripe {}/{} ({:.1}%)",
                stripe + 1,
                stripes,
                100.0 * (stripe + 1) as f64 / stripes as f64
            );
        }
        prog2.inc_block();
    }
    prog2.stop();

    // Append compressed index as TRAILER (PARXBV2): [zdata][u32 zlen][u32 crc32]
    for i in 0..volumes {
        let f = &mut vol_files[i];
        let bin = bincode::serialize(&vol_entries[i])?;
        let z = zstd::encode_all(std::io::Cursor::new(bin), 3)?;
        let crc = crc32fast::hash(&z);
        f.seek(SeekFrom::End(0))?;
        f.write_all(&z)?;
        f.write_all(&(z.len() as u32).to_le_bytes())?;
        f.write_all(&crc.to_le_bytes())?;
    }

    // Close FDs so rename works cleanly
    drop(vol_files);

    // Update header entries_len and rename volumes with +NNN
    for i in 0..volumes {
        let old_path = out_dir.join(vol_name(i));
        let entry_count = vol_entries[i].len();
        if let Ok(mut f) = OpenOptions::new().read(true).write(true).open(&old_path) {
            let header_new = VolumeHeaderBin {
                k: stripe_k as u32,
                m: m_per_stripe as u32,
                chunk_size: chunk_size as u32,
                total_chunks: (total_chunks) as u64,
                volume_id: i as u32,
                entries_len: u32::try_from(entry_count)?,
                manifest_hash: *mani_hash.as_bytes(),
            };
            let hdr_bytes_new = bincode::serialize(&header_new)?;
            let hdr_len_u32 = hdr_lens[i];
            if u32::try_from(hdr_bytes_new.len())? == hdr_len_u32 {
                f.seek(SeekFrom::Start(7))?;
                f.write_all(&hdr_len_u32.to_le_bytes())?;
                f.write_all(&hdr_bytes_new)?;
            } else {
                eprintln!(
                    "Warning: header size changed, skipping header update for vol {}",
                    i
                );
            }
        }
        let new_path = out_dir.join(format!("vol-{:03}+{:03}.parxv", i, entry_count));
        let _ = fs::rename(&old_path, &new_path);
    }

    eprintln!("Wrote {} volume(s) under {}", volumes, out_dir.display());
    Ok(())
}

fn quickcheck(parx_dir: &Path) -> Result<()> {
    let mut seen = 0usize;
    for entry in fs::read_dir(parx_dir)? {
        let p = entry?.path();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !(name.starts_with("vol-") && name.ends_with(".parxv")) {
            continue;
        }
        seen += 1;

        let mut f = match File::open(&p) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{name}: open ERROR ({e})");
                continue;
            }
        };

        // Header
        let mut magic = [0u8; 7];
        if f.read_exact(&mut magic).is_err() || (&magic != b"PARXBV1" && &magic != b"PARXBV2") {
            eprintln!("{name}: bad magic / header");
            continue;
        }
        let v2 = &magic == b"PARXBV2";

        let mut lenb = [0u8; 4];
        if f.read_exact(&mut lenb).is_err() {
            eprintln!("{name}: header length read ERROR");
            continue;
        }
        let hdr_len = u32::from_le_bytes(lenb) as usize;
        let mut hdrb = vec![0u8; hdr_len];
        if f.read_exact(&mut hdrb).is_err() {
            eprintln!("{name}: header payload read ERROR");
            continue;
        }

        let header: VolumeHeaderBin = match bincode::deserialize(&hdrb) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("{name}: header decode ERROR ({e})");
                continue;
            }
        };

        match read_volume_index(&mut f, hdr_len, v2) {
            Ok(ents) => {
                eprintln!(
                    "{name}: K={} M={} entries={} (hdr_count={})",
                    header.k,
                    header.m,
                    ents.len(),
                    header.entries_len
                );
            }
            Err(e) => {
                eprintln!("{name}: index ERROR ({e}); hdr_count={}", header.entries_len);
            }
        }
    }
    if seen == 0 {
        return Err(anyhow!("no volumes found under {}", parx_dir.display()));
    }
    Ok(())
}

fn verify(manifest_path: &Path, root: &Path) -> Result<()> {
    let mani: Manifest = serde_json::from_reader(File::open(manifest_path)?)?;
    let (ok, bad, root_ok) = hash_check(&mani, root)?;
    eprintln!(
        "Chunks ok={}, bad={}; Merkle={}",
        ok,
        bad,
        if root_ok { "OK" } else { "MISMATCH" }
    );
    if bad == 0 && root_ok {
        println!("OK");
    } else {
        println!("BAD");
    }
    Ok(())
}

fn audit(manifest_path: &Path, root: &Path) -> Result<()> {
    let mani: Manifest = serde_json::from_reader(File::open(manifest_path)?)?;
    let (_ok, _bad, _root_ok) = hash_check(&mani, root)?;
    let stripes = (mani.total_chunks as usize).div_ceil(mani.stripe_k);
    let mut counts = vec![0usize; stripes];
    for fe in &mani.files {
        let p = root.join(&fe.rel_path);
        if !p.exists() {
            for ch in &fe.chunks {
                counts[(ch.idx as usize) / mani.stripe_k] += 1;
            }
            continue;
        }
        let f = File::open(&p)?;
        let mmap = unsafe { Mmap::map(&f)? };
        for ch in &fe.chunks {
            let st = ch.file_offset as usize;
            let en = (st + ch.len as usize).min(mmap.len());
            let dig = blake3::hash(&mmap[st..en]);
            if hex(dig.as_bytes()) != ch.hash_hex {
                counts[(ch.idx as usize) / mani.stripe_k] += 1;
            }
        }
    }
    let total_bad: usize = counts.iter().sum();
    let m_per_stripe = ((mani.parity_pct as f64 / 100.0) * (mani.stripe_k as f64))
        .round()
        .max(1.0) as usize;
    let worst = counts.iter().copied().max().unwrap_or(0);
    println!("Bad chunks total: {}", total_bad);
    println!(
        "Worst stripe damage: {} (parity per stripe M={})",
        worst, m_per_stripe
    );
    if worst <= m_per_stripe {
        println!("Repairable: YES");
    } else {
        println!("Repairable: NO (need {} more in worst stripe)", worst - m_per_stripe);
    }
    Ok(())
}

fn paritycheck(parx_dir: &Path) -> Result<()> {
    let mut per_stripe_counts: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut per_outer_counts: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut vol_reports: Vec<(String, usize, &'static str)> = Vec::new();

    for entry in fs::read_dir(parx_dir)? {
        let p = entry?.path();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !(name.starts_with("vol-") && name.ends_with(".parxv")) {
            continue;
        }

        let mut f = match File::open(&p) {
            Ok(f) => f,
            Err(_) => {
                vol_reports.push((name, 0, "OPEN_ERROR"));
                continue;
            }
        };

        // Header
        let mut magic = [0u8; 7];
        if f.read_exact(&mut magic).is_err() || (&magic != b"PARXBV1" && &magic != b"PARXBV2") {
            vol_reports.push((name, 0, "BAD_HEADER"));
            continue;
        }
        let v2 = &magic == b"PARXBV2";

        let mut lenb = [0u8; 4];
        if f.read_exact(&mut lenb).is_err() {
            vol_reports.push((name, 0, "HDRLEN_ERROR"));
            continue;
        }
        let hdr_len = u32::from_le_bytes(lenb) as usize;
        let mut hdrb = vec![0u8; hdr_len];
        if f.read_exact(&mut hdrb).is_err() {
            vol_reports.push((name, 0, "HDRPAYLOAD_ERROR"));
            continue;
        }
        let header: VolumeHeaderBin = match bincode::deserialize(&hdrb) {
            Ok(h) => h,
            Err(_) => {
                vol_reports.push((name, 0, "HDRDECODE_ERROR"));
                continue;
            }
        };

        let entries = read_volume_index(&mut f, hdr_len, v2).unwrap_or_default();

        let mut present_here = 0usize;
        for e in &entries {
            if e.stripe != u32::MAX {
                let entry = per_stripe_counts.entry(e.stripe).or_insert((0, 0));
                entry.0 += 1;
                if let Some(h) = e.hash {
                    if let Ok(Some(buf)) = safe_read_exact_at(&mut f, e.offset, e.len as usize) {
                        if *blake3::hash(&buf).as_bytes() == h {
                            entry.1 += 1;
                            present_here += 1;
                        }
                    }
                }
            } else if let Some(s) = e.outer_for_stripe {
                let entry = per_outer_counts.entry(s).or_insert((0, 0));
                entry.0 += 1;
                if let Some(h) = e.hash {
                    if let Ok(Some(buf)) = safe_read_exact_at(&mut f, e.offset, e.len as usize) {
                        if *blake3::hash(&buf).as_bytes() == h {
                            entry.1 += 1;
                            present_here += 1;
                        }
                    }
                }
            }
        }
        let count = if !entries.is_empty() {
            entries.len()
        } else {
            header.entries_len as usize
        };
        vol_reports.push((name, count, "OK"));
    }

    if vol_reports.is_empty() {
        return Err(anyhow!("no parity volumes found under {}", parx_dir.display()));
    }

    println!("Parity audit across {} volume(s):", vol_reports.len());
    for (name, ents, status) in vol_reports {
        println!("  {:20}  entries {:5}   index: {}", name, ents, status);
    }

    let mut stripes: Vec<_> = per_stripe_counts.into_iter().collect();
    stripes.sort_by_key(|(s, _)| *s);
    if !stripes.is_empty() {
        for (s, (present, verified)) in stripes {
            println!(
                "  stripe {:6}: inner present {:3}, verified {:3}",
                s, present, verified
            );
        }
    }
    let mut outers: Vec<_> = per_outer_counts.into_iter().collect();
    outers.sort_by_key(|(s, _)| *s);
    if !outers.is_empty() {
        for (s, (present, verified)) in outers {
            println!(
                "  stripe {:6}: outer present {:3}, verified {:3}",
                s, present, verified
            );
        }
    }
    Ok(())
}

fn repair(manifest_path: &Path, root: &Path) -> Result<()> {
    let mani: Manifest = serde_json::from_reader(File::open(manifest_path)?)?;
    let parx_dir = Path::new(manifest_path).parent().unwrap_or_else(|| Path::new("."));

    // chunk map
    let mut map: Vec<(PathBuf, u64, u32, String)> =
        vec![(PathBuf::new(), 0, 0, String::new()); mani.total_chunks as usize];
    for fe in &mani.files {
        let rp = PathBuf::from(&fe.rel_path);
        for ch in &fe.chunks {
            map[ch.idx as usize] = (rp.clone(), ch.file_offset, ch.len, ch.hash_hex.clone());
        }
    }

    // detect damaged chunks
    let mut bad: HashSet<usize> = HashSet::new();
    for (idx, (rp, off, len, hexexp)) in map.iter().enumerate() {
        let p = root.join(rp);
        let mut good = false;
        if p.exists() {
            if let Ok(f) = File::open(&p) {
                let mmap = unsafe { Mmap::map(&f)? };
                let st = *off as usize;
                let en = (st + *len as usize).min(mmap.len());
                if en > st {
                    let dig = blake3::hash(&mmap[st..en]);
                    good = hex(dig.as_bytes()) == *hexexp;
                }
            }
        }
        if !good {
            bad.insert(idx);
        }
    }
    if bad.is_empty() {
        println!("Nothing to repair");
        return Ok(());
    }

    // Load all volume indices
    let mut vol_files: Vec<File> = vec![];
    let mut vol_entries_all: Vec<Vec<VolumeEntry>> = vec![];
    for entry in fs::read_dir(parx_dir)? {
        let p = entry?.path();
        let ok_name = p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with("vol-") && s.ends_with(".parxv"))
            .unwrap_or(false);
        if !ok_name {
            continue;
        }
        let mut f = File::open(&p)?;
        let mut magic = [0u8; 7];
        if f.read_exact(&mut magic).is_err() || (&magic != b"PARXBV1" && &magic != b"PARXBV2") {
            continue;
        }
        let v2 = &magic == b"PARXBV2";
        let mut lenb = [0u8; 4];
        if f.read_exact(&mut lenb).is_err() {
            continue;
        }
        let hdr_len = u32::from_le_bytes(lenb) as usize;
        let mut hdrb = vec![0u8; hdr_len];
        if f.read_exact(&mut hdrb).is_err() {
            continue;
        }
        let _header: VolumeHeaderBin = match bincode::deserialize(&hdrb) {
            Ok(h) => h,
            Err(_) => continue,
        };
        let entries = read_volume_index(&mut f, hdr_len, v2).unwrap_or_default();
        vol_files.push(f);
        vol_entries_all.push(entries);
    }
    if vol_files.is_empty() {
        return Err(anyhow!("no volumes found"));
    }

    let k_cfg = mani.stripe_k;
    let m_per_stripe = ((mani.parity_pct as f64 / 100.0) * (mani.stripe_k as f64))
        .round()
        .max(1.0) as usize;
    let outer_m = mani.outer_parity;
    let chunk_size = mani.chunk_size;
    let stripes = (mani.total_chunks as usize).div_ceil(k_cfg);

    // Build index: per stripe -> list of available inner parity shards (vi, offset, parity_idx, hash)
    // and outer parity shards (vi, offset, outer_idx, hash) mapped by stripe
    type InnerE = (usize, u64, u16, Option<[u8; 32]>);
    type OuterE = (usize, u64, u16, Option<[u8; 32]>);
    let mut inner_idx: Vec<Vec<InnerE>> = vec![vec![]; stripes];
    let mut outer_idx: Vec<Vec<OuterE>> = vec![vec![]; stripes];
    for (vi, ents) in vol_entries_all.iter().enumerate() {
        for e in ents {
            if e.stripe != u32::MAX {
                inner_idx[e.stripe as usize].push((vi, e.offset, e.parity_idx, e.hash));
            } else if let Some(s) = e.outer_for_stripe {
                // outer parity shard
                outer_idx[s as usize].push((vi, e.offset, e.parity_idx, e.hash));
            }
        }
    }

    let mut repaired_total = 0usize;

    for s in 0..stripes {
        let start = s * k_cfg;
        let end = ((s + 1) * k_cfg).min(mani.total_chunks as usize);
        let k_active = end - start;

        let missing: Vec<usize> = (start..end).filter(|i| bad.contains(i)).collect();
        if missing.is_empty() {
            continue;
        }

        // Data + parity shards for inner reconstruction
        let mut shards: Vec<Option<Vec<u8>>> = vec![None; k_active + m_per_stripe];

        // fill known data
        for gi in start..end {
            if !bad.contains(&gi) {
                let (rp, off, len, _) = &map[gi];
                let p = root.join(rp);
                let f = File::open(&p)?;
                let mmap = unsafe { Mmap::map(&f)? };
                let st = *off as usize;
                let en = st + (*len as usize);
                let mut v = vec![0u8; chunk_size];
                v[..*len as usize].copy_from_slice(&mmap[st..en]);
                if *len as usize != chunk_size {
                    v[*len as usize..].fill(0);
                }
                shards[gi - start] = Some(v);
            }
        }

        // gather inner parity by index
        let mut inner_pars: Vec<Option<Vec<u8>>> = vec![None; m_per_stripe];
        let mut got_inner = 0usize;
        for (vi, off, pi, opt_h) in &inner_idx[s] {
            if let Ok(Some(v)) = safe_read_exact_at(&mut vol_files[*vi], *off, chunk_size) {
                if let Some(h) = opt_h {
                    if *blake3::hash(&v).as_bytes() != *h {
                        continue;
                    }
                }
                let idx = (*pi) as usize;
                if idx < m_per_stripe && inner_pars[idx].is_none() {
                    inner_pars[idx] = Some(v);
                    got_inner += 1;
                }
            }
        }

        let needed = missing.len();
        if got_inner < needed && outer_m > 0 {
            // Try outer reconstruction per stripe
            let outer_m_usize = outer_m as usize;
            let mut rec: Vec<Option<Vec<u8>>> = vec![None; m_per_stripe + outer_m_usize];
            for i in 0..m_per_stripe {
                rec[i] = inner_pars[i].clone();
            }
            // load outer shards by their parity_idx
            for (vi, off, oi, opt_h) in &outer_idx[s] {
                if let Ok(Some(v)) = safe_read_exact_at(&mut vol_files[*vi], *off, chunk_size) {
                    if let Some(h) = opt_h {
                        if *blake3::hash(&v).as_bytes() != *h {
                            continue;
                        }
                    }
                    let oidx = (*oi) as usize;
                    if oidx < outer_m_usize && rec[m_per_stripe + oidx].is_none() {
                        rec[m_per_stripe + oidx] = Some(v);
                    }
                }
            }
            // Only attempt if we have enough total shards
            let have = rec.iter().filter(|o| o.is_some()).count();
            if have >= m_per_stripe {
                let rs_outer = RsCodec::new(m_per_stripe, outer_m_usize)?;
                rs_outer.reconstruct(&mut rec)?;
                // fill inner_pars
                for i in 0..m_per_stripe {
                    if inner_pars[i].is_none() {
                        inner_pars[i] = rec[i].clone();
                        if inner_pars[i].is_some() {
                            got_inner += 1;
                        }
                    }
                }
            }
        }

        // Place inner parity into shards (after outer attempt)
        for i in 0..m_per_stripe {
            if let Some(ref v) = inner_pars[i] {
                shards[k_active + i] = Some(v.clone());
            }
        }

        if got_inner < needed {
            eprintln!(
                "Stripe {} usable parity {} < needed {}; cannot repair this stripe",
                s, got_inner, needed
            );
            continue;
        }

        let rs = RsCodec::new(k_active, m_per_stripe)?;
        rs.reconstruct(&mut shards)?;

        for gi in missing {
            let local = gi - start;
            let buf = shards[local].as_ref().unwrap();
            let (rp, off, len, hexexp) = &map[gi];
            let p = root.join(rp);
            if !p.exists() {
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent).ok();
                }
                let f = File::create(&p)?;
                f.set_len(off + *len as u64)?;
                drop(f);
            }
            let mut f = File::options().read(true).write(true).open(&p)?;
            f.seek(SeekFrom::Start(*off))?;
            f.write_all(&buf[..*len as usize])?;
            let got_hex = hex(blake3::hash(&buf[..*len as usize]).as_bytes());
            if got_hex == *hexexp {
                repaired_total += 1;
                eprintln!("Repaired chunk {} (stripe {})", gi, s);
            } else {
                eprintln!("Warning: reconstructed chunk {} hash mismatch", gi);
            }
        }
    }

    println!("Repaired {} chunks", repaired_total);
    Ok(())
}

// ---- Safe I/O helpers ----

fn safe_read_exact_at(f: &mut File, off: u64, len: usize) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::ErrorKind;
    let flen = f.metadata()?.len();
    if off > flen {
        return Ok(None);
    }
    if off.saturating_add(len as u64) > flen {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(off))?;
    let mut buf = vec![0u8; len];
    match f.read_exact(&mut buf) {
        Ok(()) => Ok(Some(buf)),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_volume_index(f: &mut File, hdr_len: usize, v2: bool) -> Result<Vec<VolumeEntry>> {
    let flen = f.metadata()?.len();
    if flen < 4 {
        return Ok(Vec::new());
    }

    // after magic(7) + hdrlen(4) + header
    let after_hdr = 7 + 4 + hdr_len as u64;

    // Try inline layout first if we have room for zlen (and CRC if v2)
    let inline_possible = if v2 { flen >= after_hdr + 8 } else { flen >= after_hdr + 4 };
    let mut zb: Vec<u8> = Vec::new();

    if inline_possible {
        f.seek(SeekFrom::Start(after_hdr))?;
        if v2 {
            let mut zlenb = [0u8; 4];
            let mut crcb = [0u8; 4];
            if f.read_exact(&mut zlenb).is_ok() && f.read_exact(&mut crcb).is_ok() {
                let zlen = u32::from_le_bytes(zlenb) as u64;
                let crc_expected = u32::from_le_bytes(crcb);
                let zstart = after_hdr + 8;
                if zlen > 0 && zstart.saturating_add(zlen) <= flen {
                    if let Ok(Some(buf)) = safe_read_exact_at(f, zstart, zlen as usize) {
                        if crc32fast::hash(&buf) == crc_expected {
                            zb = buf;
                        }
                    }
                }
            }
        } else {
            let mut zlenb = [0u8; 4];
            if f.read_exact(&mut zlenb).is_ok() {
                let zlen = u32::from_le_bytes(zlenb) as u64;
                let zstart = after_hdr + 4;
                if zlen > 0 && zstart.saturating_add(zlen) <= flen {
                    if let Ok(Some(buf)) = safe_read_exact_at(f, zstart, zlen as usize) {
                        zb = buf;
                    }
                }
            }
        }
    }

    // Fallback to trailer layout
    if zb.is_empty() {
        if v2 {
            if flen < 8 {
                return Ok(Vec::new());
            }
            let mut zlenb = [0u8; 4];
            let mut crcb = [0u8; 4];
            f.seek(SeekFrom::End(-8))?;
            if f.read_exact(&mut zlenb).is_err() || f.read_exact(&mut crcb).is_err() {
                return Ok(Vec::new());
            }
            let zlen = u32::from_le_bytes(zlenb) as u64;
            let crc_expected = u32::from_le_bytes(crcb);
            if zlen == 0 || zlen + 8 > flen {
                return Ok(Vec::new());
            }
            let zstart = flen - 8 - zlen;
            if let Ok(Some(buf)) = safe_read_exact_at(f, zstart, zlen as usize) {
                if crc32fast::hash(&buf) == crc_expected {
                    zb = buf;
                } else {
                    return Err(anyhow!("index CRC mismatch").into());
                }
            } else {
                return Ok(Vec::new());
            }
        } else {
            if flen < 4 {
                return Ok(Vec::new());
            }
            let mut zlenb = [0u8; 4];
            f.seek(SeekFrom::End(-4))?;
            if f.read_exact(&mut zlenb).is_err() {
                return Ok(Vec::new());
            }
            let zlen = u32::from_le_bytes(zlenb) as u64;
            if zlen == 0 || zlen + 4 > flen {
                return Ok(Vec::new());
            }
            let zstart = flen - 4 - zlen;
            if let Ok(Some(buf)) = safe_read_exact_at(f, zstart, zlen as usize) {
                zb = buf;
            } else {
                return Ok(Vec::new());
            }
        }
    }

    let de = zstd::decode_all(std::io::Cursor::new(zb))?;
    // Try V2 entries; fallback to V1
    match parx_core::volume::decode_entries_anyver(&de) {
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
    }
}

// -------------------------

fn hash_check(mani: &Manifest, root: &Path) -> Result<(u64, u64, bool)> {
    let mut ok = 0u64;
    let mut bad = 0u64;
    let mut leaves: Vec<[u8; 32]> = Vec::with_capacity(mani.total_chunks as usize);
    for fe in &mani.files {
        let p = root.join(&fe.rel_path);
        if !p.exists() {
            for _ in &fe.chunks {
                bad += 1;
                leaves.push([0u8; 32]);
            }
            continue;
        }
        let f = File::open(&p)?;
        let mmap = unsafe { Mmap::map(&f)? };
        for ch in &fe.chunks {
            let st = ch.file_offset as usize;
            let en = (st + ch.len as usize).min(mmap.len());
            let dig = blake3::hash(&mmap[st..en]);
            if hex(dig.as_bytes()) == ch.hash_hex {
                ok += 1;
            } else {
                bad += 1;
            }
            leaves.push(*dig.as_bytes());
        }
    }
    let root_calc = merkle_root_blake3(&leaves);
    Ok((ok, bad, hex(&root_calc) == mani.merkle_root_hex))
}

fn make_rel_path(p: &Path) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let rp = pathdiff::diff_paths(p, cwd).unwrap_or_else(|| p.to_path_buf());
    Ok(rp.to_string_lossy().replace('\\', "/"))
}

fn read_next_chunk(
    readers: &mut [(PathBuf, File)],
    cur_file: &mut usize,
    buf: &mut [u8],
) -> Result<usize> {
    loop {
        if *cur_file >= readers.len() {
            return Ok(0);
        }
        let (_, f) = &mut readers[*cur_file];
        let n = f.read(buf)?;
        if n == 0 {
            *cur_file += 1;
            continue;
        } else {
            return Ok(n);
        }
    }
}
