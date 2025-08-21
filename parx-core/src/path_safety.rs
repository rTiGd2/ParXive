use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, Default)]
pub struct PathPolicy {
    pub follow_symlinks: bool,
}

/// Ensure `rel` is safe relative to `root`: no absolute, no `..`, and
/// if `follow_symlinks` then canonicalized path must stay under root; otherwise
/// warn on symlinks by returning a special error.
pub fn validate_path(root: &Path, rel: &Path, policy: PathPolicy) -> Result<PathBuf> {
    if rel.is_absolute() {
        bail!("absolute paths are not allowed: {:?}", rel);
    }
    for comp in rel.components() {
        if matches!(comp, Component::ParentDir) {
            bail!("parent traversal not allowed: {:?}", rel);
        }
    }
    let candidate = root.join(rel);
    let meta = std::fs::symlink_metadata(&candidate);
    if !policy.follow_symlinks {
        if let Ok(m) = &meta {
            if m.file_type().is_symlink() {
                bail!("symlink encountered (not following): {:?}", candidate);
            }
        }
        // Also check any ancestor components are not symlinks
        let mut cur = root.to_path_buf();
        for comp in rel.components() {
            cur = cur.join(comp);
            if let Ok(m) = std::fs::symlink_metadata(&cur) {
                if m.file_type().is_symlink() {
                    bail!("symlink in path (not following): {:?}", cur);
                }
            }
        }
        Ok(candidate)
    } else {
        let root_can = std::fs::canonicalize(root)?;
        let cand_can = std::fs::canonicalize(&candidate)?;
        if !cand_can.starts_with(&root_can) {
            bail!("path escapes root: {:?}", rel);
        }
        Ok(cand_can)
    }
}
