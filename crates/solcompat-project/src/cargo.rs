//! Cargo workspace requirements, lockfile versions, and captured metadata resolution.
use crate::resolution::{Resolutions, VersionResolution};

use crate::input::{exists, inside, label, read_text};
use anyhow::{ensure, Context, Result};
use semver::{Version, VersionReq};
use serde_json::Value as JsonValue;
use solcompat_core::Evidence;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(crate) fn cargo_workspace_root(root: &Path) -> Result<PathBuf> {
    for directory in root.ancestors() {
        let manifest = directory.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = read_text(&manifest)?;
        let document: toml::Value = toml::from_str(&text)
            .with_context(|| format!("invalid Cargo manifest {}", manifest.display()))?;
        if document.get("workspace").is_some() {
            return Ok(directory.to_path_buf());
        }
    }
    Ok(root.to_path_buf())
}

#[derive(Default)]
pub(crate) struct WorkspaceRequirements {
    pub(crate) dependencies: BTreeMap<String, toml::Value>,
    pub(crate) evidence: Option<Evidence>,
}

pub(crate) fn workspace_requirements(
    root: &Path,
    selected: &Path,
) -> Result<WorkspaceRequirements> {
    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(WorkspaceRequirements::default());
    }
    let text = read_text(&manifest)?;
    let document: toml::Value = toml::from_str(&text)
        .with_context(|| format!("invalid Cargo manifest {}", manifest.display()))?;
    let dependencies = document
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table)
        .map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    Ok(WorkspaceRequirements {
        dependencies,
        evidence: Some(crate::input::workspace_evidence(
            selected,
            &manifest,
            "/workspace/dependencies",
            text.as_bytes(),
        )),
    })
}

pub(crate) fn inherit_dependencies(
    document: &toml::Value,
    workspace: &WorkspaceRequirements,
) -> (toml::Value, bool) {
    fn inherit(
        table: Option<&mut toml::value::Table>,
        workspace: &WorkspaceRequirements,
        used: &mut bool,
    ) {
        let Some(table) = table else {
            return;
        };
        for (alias, value) in table {
            if value.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
                continue;
            }
            let Some(inherited) = workspace.dependencies.get(alias) else {
                continue;
            };
            let mut combined = match inherited {
                toml::Value::String(version) => toml::value::Table::from_iter([(
                    "version".into(),
                    toml::Value::String(version.clone()),
                )]),
                toml::Value::Table(table) => table.clone(),
                _ => continue,
            };
            if let Some(local) = value.as_table() {
                for (key, val) in local {
                    if key != "workspace" {
                        combined.insert(key.clone(), val.clone());
                    }
                }
            }
            *value = toml::Value::Table(combined);
            *used = true;
        }
    }
    let mut document = document.clone();
    let mut used = false;
    for section in ["dependencies", "build-dependencies"] {
        inherit(
            document
                .get_mut(section)
                .and_then(toml::Value::as_table_mut),
            workspace,
            &mut used,
        );
    }
    if let Some(targets) = document
        .get_mut("target")
        .and_then(toml::Value::as_table_mut)
    {
        for (_, target) in targets.iter_mut() {
            inherit(
                target
                    .get_mut("dependencies")
                    .and_then(toml::Value::as_table_mut),
                workspace,
                &mut used,
            );
        }
    }
    (document, used)
}

/// Only the canonical crates.io registry identity establishes reviewed upstream versions.
fn crates_io(source: Option<&str>) -> bool {
    matches!(
        source,
        Some(
            "registry+https://github.com/rust-lang/crates.io-index"
                | "sparse+https://index.crates.io/"
                | "registry+sparse+https://index.crates.io/"
        )
    )
}

#[derive(Default)]
pub(crate) struct LockSnapshot {
    pub(crate) versions: Resolutions,
    packages: Vec<toml::Value>,
    pub(crate) evidence: Option<Evidence>,
}

pub(crate) fn lock_versions(root: &Path, selected: &Path) -> Result<LockSnapshot> {
    let path = root.join("Cargo.lock");
    if !exists(&path)? {
        return Ok(LockSnapshot::default());
    }
    let text = read_text(&path)?;
    let doc: toml::Value = toml::from_str(&text).context("invalid Cargo.lock")?;
    let packages = doc
        .get("package")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in &packages {
        if !crates_io(package.get("source").and_then(toml::Value::as_str)) {
            continue;
        }
        if let (Some(name), Some(version)) = (
            package.get("name").and_then(toml::Value::as_str),
            package.get("version").and_then(toml::Value::as_str),
        ) {
            out.entry(name.into()).or_default().insert(version.into());
        }
    }
    Ok(LockSnapshot {
        versions: out
            .into_iter()
            .map(|(name, versions)| (name, VersionResolution::CargoLock(versions)))
            .collect(),
        packages,
        evidence: Some(crate::input::workspace_evidence(
            selected,
            &path,
            "/package",
            text.as_bytes(),
        )),
    })
}

impl LockSnapshot {
    /// Resolve a normal required registry dependency through this package's lock edges.
    /// Optional/conditional/build-only, local/Git/fork, missing-owner, or ambiguous edges
    /// need captured metadata rather than borrowing another package's version.
    pub(crate) fn program_resolutions(&self, document: &toml::Value) -> Resolutions {
        let name = document
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str);
        let version = document
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(toml::Value::as_str);
        let owners: Vec<_> = self
            .packages
            .iter()
            .filter(|p| {
                p.get("name").and_then(toml::Value::as_str) == name
                    && p.get("source").is_none()
                    && version
                        .is_none_or(|v| p.get("version").and_then(toml::Value::as_str) == Some(v))
            })
            .collect();
        if owners.len() != 1 {
            return Resolutions::new();
        }
        let edges: Vec<_> = owners[0]
            .get("dependencies")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str)
            .collect();
        let mut out = Resolutions::new();
        let mut unsupported_packages = BTreeSet::new();
        if let Some(deps) = document.get("dependencies").and_then(toml::Value::as_table) {
            for (alias, dep) in deps {
                let package = dep
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias);
                let unsupported = ["git", "path", "registry", "workspace"]
                    .iter()
                    .any(|key| dep.get(*key).is_some())
                    || dep.get("optional").and_then(toml::Value::as_bool) == Some(true);
                if unsupported {
                    unsupported_packages.insert(package.to_owned());
                    continue;
                }
                let requirement = dep
                    .as_str()
                    .or_else(|| dep.get("version").and_then(toml::Value::as_str))
                    .and_then(|r| VersionReq::parse(r).ok());
                let Some(requirement) = requirement else {
                    continue;
                };
                let mut candidates = BTreeSet::new();
                if self.versions.contains_key(package) {
                    for edge in &edges {
                        let fields: Vec<_> = edge.split_whitespace().collect();
                        if fields.first() != Some(&package) {
                            continue;
                        }
                        let linked: Vec<_> = self
                            .packages
                            .iter()
                            .filter(|record| {
                                record.get("name").and_then(toml::Value::as_str) == Some(package)
                                    && fields.get(1).is_none_or(|version| {
                                        record.get("version").and_then(toml::Value::as_str)
                                            == Some(*version)
                                    })
                                    && fields.get(2).is_none_or(|source| {
                                        record.get("source").and_then(toml::Value::as_str)
                                            == Some(source.trim_matches(['(', ')']))
                                    })
                            })
                            .collect();
                        if linked.len() != 1
                            || !crates_io(linked[0].get("source").and_then(toml::Value::as_str))
                        {
                            continue;
                        }
                        if let Some(version) =
                            linked[0].get("version").and_then(toml::Value::as_str)
                        {
                            if Version::parse(version).is_ok_and(|v| requirement.matches(&v)) {
                                candidates.insert(version.into());
                            }
                        }
                    }
                }
                match out.entry(package.into()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(VersionResolution::CargoLock(candidates));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if let VersionResolution::CargoLock(existing) = entry.get_mut() {
                            existing.extend(candidates);
                        }
                    }
                }
            }
        }
        for package in unsupported_packages {
            out.remove(&package);
        }
        out
    }
}

pub(crate) fn cargo_deps(
    table: Option<&toml::value::Table>,
    prefix: &str,
    out: &mut BTreeMap<String, String>,
) {
    let Some(table) = table else { return };
    for (name, value) in table {
        let req = value
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                value
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                if value.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                    "workspace".into()
                } else if value.get("git").is_some() {
                    "git".into()
                } else if value.get("path").is_some() {
                    "path".into()
                } else {
                    "unspecified".into()
                }
            });
        let identity = value
            .get("package")
            .and_then(toml::Value::as_str)
            .map_or_else(|| name.clone(), |package| format!("{name}=>{package}"));
        out.insert(format!("{prefix}/{identity}"), req);
    }
}
pub(crate) fn cargo_dependency_names(document: &toml::Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for section in ["dependencies", "build-dependencies"] {
        if let Some(table) = document.get(section).and_then(toml::Value::as_table) {
            for (alias, value) in table {
                names.insert(
                    value
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(alias)
                        .to_owned(),
                );
            }
        }
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            if let Some(table) = target.get("dependencies").and_then(toml::Value::as_table) {
                for (alias, value) in table {
                    names.insert(
                        value
                            .get("package")
                            .and_then(toml::Value::as_str)
                            .unwrap_or(alias)
                            .to_owned(),
                    );
                }
            }
        }
    }
    names
}
pub(crate) fn required_cargo_dependency_names(document: &toml::Value) -> BTreeSet<String> {
    fn add(table: Option<&toml::value::Table>, names: &mut BTreeSet<String>) {
        let Some(table) = table else { return };
        for (alias, value) in table {
            if value
                .get("optional")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            names.insert(
                value
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias)
                    .to_owned(),
            );
        }
    }

    let mut names = BTreeSet::new();
    add(
        document.get("dependencies").and_then(toml::Value::as_table),
        &mut names,
    );
    names
}

pub(crate) fn metadata_resolutions(
    root: &Path,
    value: &JsonValue,
) -> Result<BTreeMap<String, Resolutions>> {
    let packages = value
        .get("packages")
        .and_then(JsonValue::as_array)
        .context("cargo metadata packages are required")?;
    let mut identities = BTreeMap::new();
    let mut manifests = BTreeMap::new();
    for package in packages {
        let id = package
            .get("id")
            .and_then(JsonValue::as_str)
            .context("metadata package id is required")?;
        let name = package
            .get("name")
            .and_then(JsonValue::as_str)
            .context("metadata package name is required")?;
        let version = package
            .get("version")
            .and_then(JsonValue::as_str)
            .context("metadata package version is required")?;
        let trusted = crates_io(package.get("source").and_then(JsonValue::as_str));
        ensure!(
            identities
                .insert(
                    id.to_owned(),
                    (name.to_owned(), version.to_owned(), trusted)
                )
                .is_none(),
            "duplicate Cargo metadata package id: {id}"
        );
        if let Some(path) = package.get("manifest_path").and_then(JsonValue::as_str) {
            let path = PathBuf::from(path);
            if let Ok(path) = inside(root, &path) {
                manifests.insert(id.to_owned(), label(root, &path));
            }
        }
    }
    let mut out = BTreeMap::new();
    if let Some(nodes) = value
        .pointer("/resolve/nodes")
        .and_then(JsonValue::as_array)
    {
        for node in nodes {
            let Some(id) = node.get("id").and_then(JsonValue::as_str) else {
                continue;
            };
            let Some(manifest) = manifests.get(id) else {
                continue;
            };
            let mut deps = BTreeMap::new();
            if let Some(edges) = node.get("deps").and_then(JsonValue::as_array) {
                for edge in edges {
                    let normal = edge
                        .get("dep_kinds")
                        .and_then(JsonValue::as_array)
                        .is_some_and(|kinds| {
                            kinds.iter().any(|kind| {
                                kind.get("kind").is_some_and(JsonValue::is_null)
                                    && kind.get("target").is_some_and(JsonValue::is_null)
                            })
                        });
                    if !normal {
                        continue;
                    }
                    if let Some((name, version, trusted)) = edge
                        .get("pkg")
                        .and_then(JsonValue::as_str)
                        .and_then(|pkg| identities.get(pkg))
                    {
                        if !trusted {
                            deps.insert(name.clone(), VersionResolution::Missing);
                            continue;
                        }
                        match deps.entry(name.clone()) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(VersionResolution::CargoMetadata(version.clone()));
                            }
                            std::collections::btree_map::Entry::Occupied(mut entry) => {
                                if entry.get().exact() != Some(version.as_str()) {
                                    entry.insert(VersionResolution::Missing);
                                }
                            }
                        }
                    }
                }
            }
            ensure!(
                out.insert(manifest.clone(), deps).is_none(),
                "duplicate Cargo metadata resolution for {manifest}"
            );
        }
    }
    Ok(out)
}
