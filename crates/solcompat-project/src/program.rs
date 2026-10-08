//! Construct Program records from manifests, dependency resolution, and Rust source observations.

use crate::{
    cargo::{cargo_dependency_names, cargo_deps, required_cargo_dependency_names},
    input::{evidence, label, read_text},
    source::rust::{has_program_entrypoint, program_source_signals},
};
use anyhow::{Context, Result};
use solcompat_core::Program;
use std::{collections::BTreeMap, path::Path};

/// Construct the initial Program from its Cargo manifest, available resolution,
/// and framework source signals. Tool selections start absent and are enriched
/// by collect.rs, then optionally by the CLI. Automatic discovery also requires
/// a recognized entrypoint; explicit bindings bypass that entrypoint test.
pub(crate) fn collect_program(
    root: &Path,
    path: &Path,
    locks: &crate::cargo::LockSnapshot,
    workspace_requirements: &crate::cargo::WorkspaceRequirements,
    id: String,
    explicit: bool,
) -> Result<Option<Program>> {
    let text = read_text(path)?;
    let doc: toml::Value = toml::from_str(&text)
        .with_context(|| format!("invalid Cargo manifest {}", path.display()))?;
    let (doc, inherited) = crate::cargo::inherit_dependencies(&doc, workspace_requirements);
    let package = match doc.get("package").and_then(|v| v.as_table()) {
        Some(p) => p,
        None => return Ok(None),
    };
    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_owned();
    let mut deps = BTreeMap::new();
    cargo_deps(
        doc.get("dependencies").and_then(|v| v.as_table()),
        "dependencies",
        &mut deps,
    );
    cargo_deps(
        doc.get("build-dependencies").and_then(|v| v.as_table()),
        "buildDependencies",
        &mut deps,
    );
    if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
        for (condition, target) in targets {
            cargo_deps(
                target.get("dependencies").and_then(toml::Value::as_table),
                &format!("target:{condition}"),
                &mut deps,
            );
        }
    }
    let names = cargo_dependency_names(&doc);
    let required_names = required_cargo_dependency_names(&doc);
    let cdylib = doc
        .get("lib")
        .and_then(|v| v.get("crate-type"))
        .and_then(|v| v.as_array())
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("cdylib")));
    if !explicit && !has_program_entrypoint(root, path, &doc)? {
        return Ok(None);
    }
    let framework = if required_names.contains("anchor-lang") {
        "anchor"
    } else if required_names.contains("pinocchio") {
        "pinocchio"
    } else if names
        .iter()
        .any(|n| n == "solana-program" || n.starts_with("solana-program-"))
    {
        "native"
    } else if cdylib {
        "custom"
    } else {
        return Ok(None);
    };
    let source_observations = program_source_signals(root, path, framework, &doc)?;
    let mut program_evidence = vec![evidence(root, path, "/", "declared", text.as_bytes())];
    if framework == "pinocchio" {
        program_evidence.push(evidence(
            root,
            path,
            "/features/default-static-profile",
            "declared",
            text.as_bytes(),
        ));
    }
    program_evidence.extend(source_observations.signals.into_values().flatten());
    program_evidence.extend(source_observations.incomplete);
    let reason = if cdylib {
        "Cargo [lib].crate-type includes cdylib"
    } else {
        "Solana program framework dependency"
    };
    if inherited {
        if let Some(ev) = &workspace_requirements.evidence {
            program_evidence.push(ev.clone());
        }
    }
    if let Some(ev) = &locks.evidence {
        program_evidence.push(ev.clone());
    }
    let observations = locks.program_resolutions(&doc);
    let resolved = crate::resolution::wire_versions(&observations);
    Ok(Some(Program {
        id,
        name,
        manifest: label(root, path),
        framework: framework.into(),
        candidate_reason: reason.into(),
        rust_version: package
            .get("rust-version")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        dependencies: deps,
        resolved_dependencies: resolved,
        anchor_cli_version: None,
        sbf_rust_version: None,
        sbpf_arch: None,
        platform_tools_version: None,
        cargo_build_sbf_version: None,
        evidence: program_evidence,
    }))
}

/// Explicit tool selections are applied before Anchor.toml and probe fallback.
pub(crate) fn apply_program_config(
    prog: &mut Program,
    c: &crate::config::ProgramConfig,
    config_ev: Option<&solcompat_core::Evidence>,
    i: usize,
) {
    prog.anchor_cli_version = c.anchor_cli_version.clone();
    prog.sbf_rust_version = c.sbf_rust_version.clone();
    prog.sbpf_arch = c.sbpf_arch.clone();
    prog.platform_tools_version = c.platform_tools_version.clone();
    prog.cargo_build_sbf_version = c.cargo_build_sbf_version.clone();
    if let Some(ev) = config_ev {
        let mut ev = ev.clone();
        ev.pointer = format!("programs[{i}]");
        prog.evidence.push(ev);
    }
}

/// A matching captured metadata node replaces the entire lockfile-derived map.
pub(crate) fn apply_metadata_resolution(
    program: &mut Program,
    dependencies: &crate::resolution::Resolutions,
    document: &toml::Value,
) {
    let mut versions = crate::resolution::wire_versions(dependencies);
    versions.retain(|package, version| {
        let declarations: Vec<_> = document
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .filter(|(alias, dep)| {
                dep.get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias)
                    == package
            })
            .collect();
        !declarations.is_empty()
            && declarations.iter().all(|(_, dep)| {
                if ["git", "path", "registry", "workspace"]
                    .iter()
                    .any(|key| dep.get(*key).is_some())
                {
                    return false;
                }
                dep.as_str()
                    .or_else(|| dep.get("version").and_then(toml::Value::as_str))
                    .and_then(|r| semver::VersionReq::parse(r).ok())
                    .is_some_and(|r| semver::Version::parse(version).is_ok_and(|v| r.matches(&v)))
            })
    });
    program.resolved_dependencies = versions;
}

/// Anchor.toml fills a missing selection but always contributes its existing evidence.
pub(crate) fn apply_anchor_selection(
    program: &mut Program,
    version: Option<&str>,
    evidence: solcompat_core::Evidence,
) {
    if program.anchor_cli_version.is_none() {
        program.anchor_cli_version = version.map(str::to_owned);
    }
    program.evidence.push(evidence);
}
