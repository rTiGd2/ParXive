use crate::index::{read_index, read_trailer, IndexLimits};
use anyhow::Result;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ParityAuditReport {
    pub volumes: usize,
    pub stripe_parity_counts: HashMap<u32, usize>,
}

/// Scan parity volumes and summarize parity entries per stripe.
pub fn audit(parity_dir: &Path) -> Result<ParityAuditReport> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let mut vols = 0usize;
    if parity_dir.exists() {
        for ent in std::fs::read_dir(parity_dir)? {
            let p = ent?.path();
            if p.extension().map(|s| s == "parxv").unwrap_or(false) {
                vols += 1;
                let mut f = File::open(&p)?;
                let (off, len, crc) = read_trailer(&mut f)?;
                let entries = read_index(&mut f, off, len, crc, &IndexLimits::default())?;
                for e in entries {
                    *counts.entry(e.stripe).or_default() += 1;
                }
            }
        }
    }
    Ok(ParityAuditReport { volumes: vols, stripe_parity_counts: counts })
}
