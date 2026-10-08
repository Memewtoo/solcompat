//! Shared version syntax; callers retain their own error classification.
use anyhow::Result;
use semver::Version;
pub(crate) fn parse_version(value: &str) -> Option<Version> {
    let value = value.trim_start_matches('v');
    Version::parse(value)
        .ok()
        .or_else(|| Version::parse(&format!("{value}.0")).ok())
}
pub(crate) fn version(value: &str, label: &str) -> Result<Version> {
    parse_version(value).ok_or_else(|| anyhow::anyhow!("invalid {label} version: {value}"))
}
