use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;

#[cfg(windows)]
fn contains_path_case_insensitive(root: &Path, child: &Path) -> bool {
    // Compare path components case-insensitively for Windows filesystems.
    // This is a best-effort normalization using lossy UTF-8 lowering.
    // UNC and verbatim prefixes are preserved as components and compared too.
    let rc: Vec<String> =
        root.components().map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase()).collect();
    let cc: Vec<String> =
        child.components().map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase()).collect();
    if rc.len() > cc.len() {
        return false;
    }
    // starts_with equivalent on lowered components
    for (i, r) in rc.iter().enumerate() {
        if cc.get(i) != Some(r) {
            return false;
        }
    }
    true
}

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
                let is_symlink = m.file_type().is_symlink();
                #[cfg(windows)]
                let is_reparse = (m.file_attributes() & 0x400) != 0; // FILE_ATTRIBUTE_REPARSE_POINT
                #[cfg(not(windows))]
                let is_reparse = false;
                if is_symlink || is_reparse {
                    bail!("symlink in path (not following): {:?}", cur);
                }
            }
        }
        Ok(candidate)
    } else {
        let root_can = std::fs::canonicalize(root)?;
        let cand_can = std::fs::canonicalize(&candidate)?;
        // On Windows, perform case-insensitive containment; elsewhere, Path::starts_with is fine.
        #[cfg(windows)]
        {
            if !contains_path_case_insensitive(&root_can, &cand_can) {
                bail!("path escapes root: {:?}", rel);
            }
        }
        #[cfg(not(windows))]
        {
            if !cand_can.starts_with(&root_can) {
                bail!("path escapes root: {:?}", rel);
            }
        }
        Ok(cand_can)
    }
}
