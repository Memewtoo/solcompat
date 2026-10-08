//! Sorted manifest discovery, excluded directories, and automatic component identities.

use crate::input::inside;
use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) const EXCLUDED: &[&str] = &[
    ".git",
    ".anchor",
    ".next",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
];

pub(crate) fn automatic_id(prefix: &str, manifest: &str) -> String {
    let stem = manifest
        .trim_end_matches("/Cargo.toml")
        .trim_end_matches("/package.json")
        .trim_end_matches("Cargo.toml")
        .trim_end_matches("package.json")
        .trim_matches('/');
    if stem.is_empty() {
        prefix.into()
    } else {
        format!(
            "{prefix}-{}",
            stem.chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                })
                .collect::<String>()
        )
    }
}
pub(crate) fn discover(root: &Path, names: &[&str], out: &mut Vec<PathBuf>) -> Result<()> {
    fn walk(root: &Path, dir: &Path, names: &[&str], out: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let ty = entry.file_type()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ty.is_symlink() {
                if names.contains(&name.as_ref()) {
                    out.push(inside(root, &entry.path())?);
                }
                continue;
            }
            if ty.is_dir() && !EXCLUDED.contains(&name.as_ref()) {
                walk(root, &entry.path(), names, out)?
            } else if ty.is_file() && names.contains(&name.as_ref()) {
                out.push(inside(root, &entry.path())?)
            }
        }
        Ok(())
    }
    walk(root, root, names, out)
}
