//! Bounded file reads, project containment, and evidence labels.

use anyhow::{ensure, Context, Result};
use solcompat_core::{digest, Evidence};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
/// Read a regular UTF-8 file with a 4 MiB limit, enforced before and during reading.
///
/// This helper does not impose project containment; callers reading bound project
/// files first use the internal containment check.
pub fn read_text(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)
            .with_context(|| format!("cannot inspect {}", path.display()))?
            .is_file(),
        "not a regular file: {}",
        path.display()
    );
    let file = fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_FILE_BYTES,
        "input exceeds 4 MiB limit: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "input exceeds 4 MiB limit: {}",
        path.display()
    );
    String::from_utf8(bytes).with_context(|| format!("input is not UTF-8: {}", path.display()))
}
pub(crate) fn read_artifact(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "not a regular file: {}", path.display());
    ensure!(
        metadata.len() <= MAX_ARTIFACT_BYTES,
        "artifact exceeds 16 MiB limit: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ARTIFACT_BYTES,
        "artifact exceeds 16 MiB limit: {}",
        path.display()
    );
    Ok(bytes)
}
pub(crate) fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn inside(root: &Path, path: &Path) -> Result<PathBuf> {
    let p = path
        .canonicalize()
        .with_context(|| format!("cannot resolve input {}", path.display()))?;
    ensure!(
        p.starts_with(root),
        "input points outside selected project root: {}",
        path.display()
    );
    Ok(p)
}
pub(crate) fn label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("inside root")
        .to_string_lossy()
        .replace('\\', "/")
}
pub(crate) fn evidence(
    root: &Path,
    path: &Path,
    pointer: &str,
    kind: &str,
    bytes: &[u8],
) -> Evidence {
    Evidence {
        path: label(root, path),
        pointer: pointer.into(),
        kind: kind.into(),
        digest: digest(bytes),
    }
}

/// Explicit label for the intentional ancestor-workspace exception.
pub(crate) fn workspace_evidence(
    root: &Path,
    path: &Path,
    pointer: &str,
    bytes: &[u8],
) -> Evidence {
    let path_label = if path.starts_with(root) {
        label(root, path)
    } else {
        let ancestor = root
            .ancestors()
            .find(|ancestor| path.starts_with(ancestor))
            .expect("shared ancestor");
        let levels = root.strip_prefix(ancestor).unwrap().components().count();
        format!(
            "external-workspace:{}{}",
            "../".repeat(levels),
            path.strip_prefix(ancestor)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        )
    };
    Evidence {
        path: path_label,
        pointer: pointer.into(),
        kind: "resolution".into(),
        digest: digest(bytes),
    }
}
