//! Inspect existing ELF headers without inferring their source or build history.

use crate::{
    config::ArtifactConfig,
    input::{evidence, inside, label, read_artifact},
};
use anyhow::{bail, ensure, Result};
use solcompat_core::Artifact;
use std::path::Path;

pub(crate) fn parse_artifact(root: &Path, c: &ArtifactConfig, base: &Path) -> Result<Artifact> {
    let p = inside(root, &base.join(&c.path))?;
    let bytes = read_artifact(&p)?;
    ensure!(
        bytes.len() >= 52 && &bytes[0..4] == b"\x7fELF",
        "artifact is not an ELF file: {}",
        p.display()
    );
    ensure!(
        bytes[5] == 1,
        "only little-endian ELF artifacts are supported"
    );
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    // Current upstream accepts legacy EM_BPF and the dedicated EM_SBPF value.
    // Newer cargo-build-sbf toolchains emit EM_SBPF even for an e_flags v0
    // artifact, so the machine and sBPF version must remain independent.
    ensure!(
        matches!(machine, 247 | 263),
        "ELF machine {machine} is not EM_BPF (247) or EM_SBPF (263)"
    );
    let flags = match bytes[4] {
        1 => u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
        2 => u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
        _ => bail!("unsupported ELF class"),
    };
    let sbpf = match flags {
        0 => Some("v0"),
        1 => Some("v1"),
        2 => Some("v2"),
        3 => Some("v3"),
        _ => None,
    }
    .map(str::to_owned);
    Ok(Artifact {
        id: c.id.clone(),
        path: label(root, &p),
        program: c.program.clone(),
        loader: c.loader.clone(),
        operation: c.operation.clone(),
        elf_machine: machine,
        elf_flags: flags,
        sbpf_version: sbpf,
        evidence: vec![evidence(root, &p, "/elf-header", "observed", &bytes)],
    })
}
