//! Collection orchestration: bindings, discovery, resolution, enrichment, and inventory assembly.

use crate::{
    artifact::parse_artifact,
    cargo::{cargo_workspace_root, lock_versions, metadata_resolutions, workspace_requirements},
    client::{collect_client, is_solana_client},
    config::{
        default_loader, default_operation, identifier, AnalysisConfig, ArtifactConfig, Config,
    },
    discovery::{automatic_id, discover},
    idl::parse_idl,
    input::{evidence, exists, inside, label, read_text},
    program::collect_program,
    source::rpc::discover_rpc_reads,
};
use anyhow::{ensure, Context, Result};
use serde_json::Value as JsonValue;
use solcompat_core::{Project, Suppression};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Inventory and policy inputs returned by [`collect`], before rule evaluation.
/// Tool observations remain empty until an application explicitly probes tools.
pub struct Collected {
    pub project: Project,
    pub suppressions: Vec<Suppression>,
    pub target: String,
}

/// Collect a local project without executing tools or project code.
///
/// Configuration bindings are applied before automatic discovery. Captured Cargo
/// metadata replaces matching lockfile resolution; Anchor.toml fills absent Anchor
/// CLI selections. Returned components are sorted by stable IDs.
///
/// Paths are constrained to the selected project, except ancestor Cargo workspace
/// manifests and lockfiles used for nested workspace resolution. Missing exact
/// versions remain unresolved; malformed or unsupported inputs return errors.
pub fn collect(
    path: &Path,
    config_path: Option<&Path>,
    target: Option<&str>,
    metadata_path: Option<&Path>,
    extra_artifacts: &[PathBuf],
) -> Result<Collected> {
    let root = path
        .canonicalize()
        .with_context(|| format!("cannot open project {}", path.display()))?;
    ensure!(root.is_dir(), "--path must select a directory");
    let default = root.join("solcompat.toml");
    let selected = if let Some(p) = config_path {
        Some(p.to_owned())
    } else if exists(&default)? {
        Some(default)
    } else {
        None
    };
    let (config, config_ev, base) = if let Some(p) = selected {
        let p = inside(&root, &p)?;
        let text = read_text(&p)?;
        let c: Config =
            toml::from_str(&text).with_context(|| format!("invalid config {}", p.display()))?;
        ensure!(c.schema_version == 1, "unsupported config schema_version");
        let ev = evidence(&root, &p, "", "asserted", text.as_bytes());
        let base = p.parent().unwrap().to_owned();
        (c, Some(ev), base)
    } else {
        (
            Config {
                schema_version: 1,
                analysis: AnalysisConfig::default(),
                clients: vec![],
                programs: vec![],
                idls: vec![],
                artifacts: vec![],
                suppressions: vec![],
            },
            None,
            root.clone(),
        )
    };
    let target = target
        .or(config.analysis.target.as_deref())
        .unwrap_or("local")
        .to_owned();
    ensure!(
        matches!(
            target.as_str(),
            "local" | "mainnet-beta-2026-09-26" | "future-sbpfv3-deployment"
        ),
        "unsupported target: {target}"
    );
    let metadata = if let Some(path) = metadata_path {
        let p = inside(&root, path)?;
        let text = read_text(&p)?;
        let v: JsonValue = serde_json::from_str(&text).context("invalid cargo metadata JSON")?;
        ensure!(
            v.get("version").and_then(|v| v.as_u64()) == Some(1),
            "cargo metadata version 1 is required"
        );
        Some((
            v,
            evidence(
                &root,
                &p,
                "/resolve/nodes",
                "metadata-resolution",
                text.as_bytes(),
            ),
        ))
    } else {
        None
    };
    let cargo_root = cargo_workspace_root(&root)?;
    let locks = lock_versions(&cargo_root, &root)?;
    let inherited_requirements = workspace_requirements(&cargo_root, &root)?;
    let mut clients = Vec::new();
    let mut client_paths = BTreeSet::new();
    let mut client_ids = BTreeSet::new();
    for (i, c) in config.clients.iter().enumerate() {
        ensure!(
            identifier(&c.id) && client_ids.insert(c.id.clone()),
            "client ids must be valid and unique"
        );
        let p = inside(&root, &base.join(&c.manifest))?;
        let mut client = collect_client(&root, &p, c.id.clone())?;
        client_paths.insert(client.manifest.clone());
        crate::client::apply_client_config(&mut client, c, config_ev.as_ref(), i)?;
        clients.push(client);
    }
    let mut manifests = Vec::new();
    discover(&root, &["package.json"], &mut manifests)?;
    for p in manifests {
        let l = label(&root, &p);
        if client_paths.contains(&l) {
            continue;
        }
        let id = automatic_id("npm", &l);
        let client = collect_client(&root, &p, id.clone())?;
        if !is_solana_client(&client) {
            continue;
        }
        ensure!(
            client_ids.insert(id.clone()),
            "automatic client id collision: {id}; bind these manifests explicitly"
        );
        clients.push(client);
    }
    let mut programs = Vec::new();
    let mut program_paths = BTreeSet::new();
    let mut program_ids = BTreeSet::new();
    for (i, c) in config.programs.iter().enumerate() {
        ensure!(
            identifier(&c.id) && program_ids.insert(c.id.clone()),
            "program ids must be valid and unique"
        );
        let p = inside(&root, &base.join(&c.manifest))?;
        let mut prog = collect_program(
            &root,
            &p,
            &locks,
            &inherited_requirements,
            c.id.clone(),
            true,
        )?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "configured program manifest has no supported program evidence: {}",
                p.display()
            )
        })?;
        program_paths.insert(prog.manifest.clone());
        crate::program::apply_program_config(&mut prog, c, config_ev.as_ref(), i);
        programs.push(prog);
    }
    let mut cargo = Vec::new();
    discover(&root, &["Cargo.toml"], &mut cargo)?;
    for p in cargo {
        let l = label(&root, &p);
        if program_paths.contains(&l) {
            continue;
        }
        let id = automatic_id("program", &l);
        if let Some(prog) = collect_program(
            &root,
            &p,
            &locks,
            &inherited_requirements,
            id.clone(),
            false,
        )? {
            ensure!(
                program_ids.insert(id.clone()),
                "automatic program id collision: {id}; bind these manifests explicitly"
            );
            programs.push(prog);
        }
    }
    if let Some((metadata, metadata_evidence)) = &metadata {
        let resolutions = metadata_resolutions(&root, metadata)?;
        for program in &mut programs {
            if let Some(dependencies) = resolutions.get(&program.manifest) {
                let document: toml::Value =
                    toml::from_str(&read_text(&root.join(&program.manifest))?)?;
                let (document, _) =
                    crate::cargo::inherit_dependencies(&document, &inherited_requirements);
                crate::program::apply_metadata_resolution(program, dependencies, &document);
                program
                    .evidence
                    .retain(|ev| ev.kind != "resolution" || ev.pointer != "/package");
                program.evidence.push(metadata_evidence.clone());
            }
        }
    }
    discover_rpc_reads(&root, &mut clients)?;
    let anchor_toml = root.join("Anchor.toml");
    if exists(&anchor_toml)? {
        let path = inside(&root, &anchor_toml)?;
        let text = read_text(&path)?;
        let document: toml::Value = toml::from_str(&text).context("invalid Anchor.toml")?;
        let anchor_version = document
            .get("toolchain")
            .and_then(|value| value.get("anchor_version"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        for program in programs
            .iter_mut()
            .filter(|program| program.framework == "anchor")
        {
            crate::program::apply_anchor_selection(
                program,
                anchor_version.as_deref(),
                evidence(&root, &path, "/toolchain", "declared", text.as_bytes()),
            );
        }
    }
    let mut idls = Vec::new();
    let mut idl_ids = BTreeSet::new();
    for c in &config.idls {
        ensure!(
            identifier(&c.id) && idl_ids.insert(c.id.clone()),
            "IDL ids must be valid and unique"
        );
        ensure!(
            clients.iter().any(|x| x.id == c.client),
            "IDL {} references unknown client {}",
            c.id,
            c.client
        );
        let client = clients.iter().find(|x| x.id == c.client).expect("checked");
        ensure!(
            client.dependencies.keys().any(|key| key
                .split_once('/')
                .is_some_and(|(_, name)| name == c.reader_package)),
            "IDL reader_package {} is not declared in {}",
            c.reader_package,
            client.manifest
        );
        idls.push(parse_idl(&root, c, &base)?);
    }
    let mut artifacts = Vec::new();
    let mut artifact_ids = BTreeSet::new();
    for c in &config.artifacts {
        ensure!(
            identifier(&c.id) && artifact_ids.insert(c.id.clone()),
            "artifact ids must be valid and unique"
        );
        ensure!(
            matches!(
                c.operation.as_str(),
                "deploy" | "upgrade" | "finalize" | "execute"
            ),
            "unsupported artifact operation"
        );
        if let Some(p) = &c.program {
            ensure!(
                programs.iter().any(|x| &x.id == p),
                "artifact references unknown program {p}"
            );
        }
        artifacts.push(parse_artifact(&root, c, &base)?);
    }
    for (i, p) in extra_artifacts.iter().enumerate() {
        let c = ArtifactConfig {
            id: format!("artifact-{}", i + 1),
            path: p.clone(),
            program: None,
            loader: default_loader(),
            operation: default_operation(),
        };
        artifacts.push(parse_artifact(&root, &c, &root)?);
    }
    ensure!(
        !clients.is_empty() || !programs.is_empty() || !artifacts.is_empty(),
        "no supported Solana program or npm client manifests found"
    );
    for s in &config.suppressions {
        ensure!(
            solcompat_core::catalog::rule(&s.rule).is_some_and(|rule| rule.suppressible),
            "unsupported suppression rule {}",
            s.rule
        );
        ensure!(
            !s.subject.trim().is_empty() && !s.reason.trim().is_empty(),
            "suppression subject and reason are required"
        );
    }
    clients.sort_by(|a, b| a.id.cmp(&b.id));
    programs.sort_by(|a, b| a.id.cmp(&b.id));
    idls.sort_by(|a, b| a.id.cmp(&b.id));
    artifacts.sort_by(|a, b| a.id.cmp(&b.id));
    let name = clients
        .iter()
        .find(|c| c.manifest == "package.json")
        .and_then(|c| c.name.clone())
        .or_else(|| programs.first().map(|p| p.name.clone()))
        .unwrap_or_else(|| "selected project".into());
    Ok(Collected {
        project: Project {
            name,
            clients,
            programs,
            idls,
            artifacts,
            tools: vec![],
        },
        suppressions: config.suppressions,
        target,
    })
}
