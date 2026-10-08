//! Opt-in executable probes and program builds; collection never calls these functions.

use crate::input::inside;
use anyhow::{bail, ensure, Context, Result};
use solcompat_core::{Program, ToolObservation};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn probe_candidate(tool: &str) -> Result<PathBuf> {
    let paths = std::env::var_os("PATH").context("PATH is unavailable")?;
    for directory in std::env::split_paths(&paths) {
        let path = directory.join(tool);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        ensure!(
            !metadata.file_type().is_symlink(),
            "refusing a symlink/shim"
        );
        ensure!(metadata.is_file(), "not a regular executable");
        let mut file = fs::File::open(&path)?;
        let mut bytes = [0u8; 4];
        let read = file.read(&mut bytes)?;
        let native = bytes[..read].starts_with(b"\x7fELF")
            || bytes.starts_with(&[0xfe, 0xed, 0xfa, 0xce])
            || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
            || bytes.starts_with(b"MZ");
        ensure!(native, "refusing a script or unrecognized launcher");
        return Ok(path);
    }
    bail!("not found")
}

/// Opt-in native executable version probes with a three-second deadline and
/// 8 KiB per-stream retention. Nonzero exits and truncated output are not version proof.
pub fn probe_tools() -> Vec<ToolObservation> {
    ["anchor", "cargo-build-sbf", "rustc"]
        .into_iter()
        .map(|tool| {
            let path = match probe_candidate(tool) {
                Ok(path) => path,
                Err(error) => {
                    return ToolObservation {
                        tool: tool.into(),
                        version: None,
                        executable: None,
                        status: format!("not probed: {error}"),
                    }
                }
            };
            let mut command = Command::new(&path);
            command.arg("--version");
            let (version, status) =
                match crate::execution::run(&mut command, Duration::from_secs(3), 8192) {
                    Ok(output) => probe_version(&output),
                    Err(error) => (None, format!("probe failed: {error}")),
                };
            ToolObservation {
                tool: tool.into(),
                version,
                executable: Some(path.to_string_lossy().into_owned()),
                status,
            }
        })
        .collect()
}

fn probe_version(output: &crate::execution::Captured) -> (Option<String>, String) {
    let failure = if output.timed_out {
        Some("timed out")
    } else if !output.success {
        Some("probe failed: unsuccessful exit or output read")
    } else if output.truncated {
        Some("probe failed: output exceeds 8 KiB retention limit")
    } else {
        None
    };
    if let Some(status) = failure {
        return (None, status.into());
    }
    let versions: std::collections::BTreeSet<_> = output
        .stdout
        .split_whitespace()
        .map(|token| token.trim_start_matches('v'))
        .filter(|token| semver::Version::parse(token).is_ok())
        .collect();
    if versions.len() != 1 {
        return (
            None,
            "probe failed: expected one exact version in stdout".into(),
        );
    }
    (
        versions.into_iter().next().map(str::to_owned),
        "observed".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_truncated_or_ambiguous_probes_do_not_supply_a_version() {
        let mut output = crate::execution::Captured {
            success: true,
            timed_out: false,
            stdout: "anchor v0.31.1".into(),
            stderr: String::new(),
            truncated: false,
        };
        assert_eq!(probe_version(&output).0.as_deref(), Some("0.31.1"));
        output.success = false;
        assert_eq!(probe_version(&output).0, None);
        output.success = true;
        output.truncated = true;
        assert_eq!(probe_version(&output).0, None);
        output.truncated = false;
        output.timed_out = true;
        assert_eq!(probe_version(&output).0, None);
        output.timed_out = false;
        output.stdout = "anchor 0.31.1 0.32.1".into();
        assert_eq!(probe_version(&output).0, None);
    }
}

#[derive(Clone, Debug)]
/// Result of an explicitly requested build, before conversion to an SC100 finding.
/// A successful build does not prove future runtime or client compatibility.
pub struct BuildObservation {
    pub success: bool,
    pub command: String,
    pub manifest: String,
    pub output: String,
}

/// Run the normal SBF build for one detected program. This is only called from
/// the explicit `--build` workflow; project discovery never executes code.
pub fn build_program(root: &Path, program: &Program) -> Result<BuildObservation> {
    build_program_with_timeout(root, program, Duration::from_secs(1800))
}

/// Run one explicitly requested build with a deadline and bounded retained output.
pub fn build_program_with_timeout(
    root: &Path,
    program: &Program,
    timeout: Duration,
) -> Result<BuildObservation> {
    let root = fs::canonicalize(root).context("cannot resolve build root")?;
    let manifest = inside(&root, &root.join(&program.manifest))?;
    let (mut command, display) = if program.framework == "anchor" {
        let mut command = Command::new("anchor");
        command.arg("build").current_dir(&root);
        (command, "anchor build".to_owned())
    } else {
        let mut command = Command::new("cargo");
        command
            .arg("build-sbf")
            .arg("--manifest-path")
            .arg(&manifest)
            .current_dir(&root);
        (
            command,
            format!("cargo build-sbf --manifest-path {}", program.manifest),
        )
    };
    let output = crate::execution::run(&mut command, timeout, 64 * 1024)
        .with_context(|| format!("failed to start `{display}`"))?;
    let mut combined = format!("{}{}", output.stdout, output.stderr);
    if output.timed_out {
        combined.push_str(&format!(
            "\nBuild timed out after {} seconds.\n",
            timeout.as_secs()
        ));
    }
    if output.truncated {
        combined.push_str("\nBuild output truncated to 64 KiB per stream.\n");
    }
    let lines: Vec<_> = combined.lines().collect();
    let start = lines.len().saturating_sub(40);
    Ok(BuildObservation {
        success: output.success,
        command: display,
        manifest: program.manifest.clone(),
        output: lines[start..].join("\n"),
    })
}

/// Apply opt-in observations without overriding explicit or manifest selections.
/// This performs no tool execution; callers decide whether to call [`probe_tools`].
/// Host rustc is retained in the inventory but never substitutes for SBF Rust.
pub fn enrich_tool_observations(
    project: &mut solcompat_core::Project,
    tools: Vec<ToolObservation>,
) {
    project.tools = tools;
    let anchor = project
        .tools
        .iter()
        .find(|tool| tool.tool == "anchor")
        .and_then(|tool| tool.version.clone());
    let builder = project
        .tools
        .iter()
        .find(|tool| tool.tool == "cargo-build-sbf")
        .and_then(|tool| tool.version.clone());
    for program in &mut project.programs {
        if program.anchor_cli_version.is_none() {
            program.anchor_cli_version = anchor.clone();
        }
        if program.cargo_build_sbf_version.is_none() {
            program.cargo_build_sbf_version = builder.clone();
        }
    }
}
