//! Bounded, file-only project discovery. Tool execution is isolated in `probe_tools` and opt-in.
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use solcompat_core::{
    digest, Artifact, Client, Evidence, IdlInput, Omitted, Program, Project, RpcMaximum, RpcRead,
    Suppression, ToolObservation,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
const EXCLUDED: &[&str] = &[
    ".git",
    ".anchor",
    ".next",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
];

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisConfig {
    target: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u32,
    #[serde(default)]
    analysis: AnalysisConfig,
    #[serde(default)]
    clients: Vec<ClientConfig>,
    #[serde(default)]
    programs: Vec<ProgramConfig>,
    #[serde(default)]
    idls: Vec<IdlConfig>,
    #[serde(default)]
    artifacts: Vec<ArtifactConfig>,
    #[serde(default)]
    suppressions: Vec<Suppression>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientConfig {
    id: String,
    manifest: PathBuf,
    decoder_package: Option<String>,
    required_read_versions: Option<Vec<String>>,
    #[serde(default)]
    rpc_reads: Vec<ReadConfig>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramConfig {
    id: String,
    manifest: PathBuf,
    anchor_cli_version: Option<String>,
    sbf_rust_version: Option<String>,
    sbpf_arch: Option<String>,
    platform_tools_version: Option<String>,
    cargo_build_sbf_version: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdlConfig {
    id: String,
    path: PathBuf,
    client: String,
    reader_package: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactConfig {
    id: String,
    path: PathBuf,
    program: Option<String>,
    #[serde(default = "default_loader")]
    loader: String,
    #[serde(default = "default_operation")]
    operation: String,
}
fn default_loader() -> String {
    "loader-v3".into()
}
fn default_operation() -> String {
    "upgrade".into()
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadConfig {
    id: Option<String>,
    method: String,
    encoding: Option<String>,
    transaction_details: Option<String>,
    max_supported_transaction_version: Option<RpcMaximum>,
}
#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    name: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: BTreeMap<String, String>,
}

pub struct Collected {
    pub project: Project,
    pub suppressions: Vec<Suppression>,
    pub target: String,
}

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
fn read_artifact(path: &Path) -> Result<Vec<u8>> {
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
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn identifier(v: &str) -> bool {
    !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn tx_version(v: &str) -> bool {
    v == "legacy"
        || v.strip_prefix('v')
            .and_then(|x| x.parse::<u8>().ok())
            .is_some_and(|n| n <= 127 && v == format!("v{n}"))
}
fn inside(root: &Path, path: &Path) -> Result<PathBuf> {
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
fn label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("inside root")
        .to_string_lossy()
        .replace('\\', "/")
}
fn automatic_id(prefix: &str, manifest: &str) -> String {
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
fn evidence(root: &Path, path: &Path, pointer: &str, kind: &str, bytes: &[u8]) -> Evidence {
    Evidence {
        path: label(root, path),
        pointer: pointer.into(),
        kind: kind.into(),
        digest: digest(bytes),
    }
}
fn discover(root: &Path, names: &[&str], out: &mut Vec<PathBuf>) -> Result<()> {
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
fn cargo_workspace_root(root: &Path) -> Result<PathBuf> {
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

fn workspace_requirements(root: &Path) -> Result<BTreeMap<String, String>> {
    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(BTreeMap::new());
    }
    let text = read_text(&manifest)?;
    let document: toml::Value = toml::from_str(&text)
        .with_context(|| format!("invalid Cargo manifest {}", manifest.display()))?;
    let mut declared = BTreeMap::new();
    cargo_deps(
        document
            .get("workspace")
            .and_then(|value| value.get("dependencies"))
            .and_then(toml::Value::as_table),
        "workspace",
        &mut declared,
    );
    Ok(declared
        .into_iter()
        .filter_map(|(key, value)| key.rsplit('/').next().map(|name| (name.to_owned(), value)))
        .collect())
}

fn lock_versions(root: &Path) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out = BTreeMap::new();
    let path = root.join("Cargo.lock");
    if !exists(&path)? {
        return Ok(out);
    }
    let text = read_text(&path)?;
    let doc: toml::Value = toml::from_str(&text).context("invalid Cargo.lock")?;
    if let Some(packages) = doc.get("package").and_then(|v| v.as_array()) {
        for p in packages {
            if let (Some(n), Some(v)) = (
                p.get("name").and_then(|v| v.as_str()),
                p.get("version").and_then(|v| v.as_str()),
            ) {
                let registry = p
                    .get("source")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|source| source.contains("crates.io"));
                if !registry {
                    continue;
                }
                out.entry(n.into())
                    .or_insert_with(BTreeSet::new)
                    .insert(v.into());
            }
        }
    }
    Ok(out)
}
fn cargo_deps(
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
fn cargo_dependency_names(document: &toml::Value) -> BTreeSet<String> {
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
fn required_cargo_dependency_names(document: &toml::Value) -> BTreeSet<String> {
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
    for section in ["dependencies", "build-dependencies"] {
        add(
            document.get(section).and_then(toml::Value::as_table),
            &mut names,
        );
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            add(
                target.get("dependencies").and_then(toml::Value::as_table),
                &mut names,
            );
        }
    }
    names
}

fn has_program_entrypoint(manifest: &Path, document: &toml::Value) -> Result<bool> {
    let directory = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cargo manifest has no parent: {}", manifest.display()))?;
    let library = document
        .get("lib")
        .and_then(|value| value.get("path"))
        .and_then(toml::Value::as_str)
        .map_or_else(|| directory.join("src/lib.rs"), |path| directory.join(path));
    let candidates = [
        library,
        directory.join("src/entrypoint.rs"),
        directory.join("src/entrypoint/mod.rs"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            let source = read_text(&candidate)?;
            if ["entrypoint!(", "program_entrypoint!(", "#[program]"]
                .iter()
                .any(|token| source.contains(token))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn rust_source_files(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !EXCLUDED.contains(&name.as_ref()) {
                rust_source_files(root, &entry.path(), out)?;
            }
        } else if file_type.is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("rs")
        {
            out.push(inside(root, &entry.path())?);
        }
    }
    Ok(())
}

fn signal_evidence(root: &Path, path: &Path, source: &str, byte_offset: usize) -> Evidence {
    let line = source[..byte_offset.min(source.len())]
        .lines()
        .count()
        .max(1);
    evidence(
        root,
        path,
        &format!("/source:{line}"),
        "observed",
        source.as_bytes(),
    )
}

fn push_signal(signals: &mut BTreeMap<String, Vec<Evidence>>, id: &str, mut evidence: Evidence) {
    evidence.kind = id.into();
    let entries = signals.entry(id.into()).or_default();
    if !entries
        .iter()
        .any(|entry| entry.path == evidence.path && entry.pointer == evidence.pointer)
    {
        entries.push(evidence);
    }
}

fn first_call_argument(source: &str, call_offset: usize) -> Option<&str> {
    let open = call_offset + source[call_offset..].find('(')?;
    let start = open + 1;
    let mut depth = 0usize;
    for (relative, character) in source[start..].char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ')' if depth == 0 => return Some(&source[start..start + relative]),
            ',' if depth == 0 => return Some(&source[start..start + relative]),
            _ => {}
        }
    }
    None
}

fn program_source_signals(
    root: &Path,
    manifest: &Path,
    framework: &str,
) -> Result<BTreeMap<String, Vec<Evidence>>> {
    let mut files = Vec::new();
    let source_directory = manifest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cargo manifest has no parent"))?
        .join("src");
    if !source_directory.is_dir() {
        return Ok(BTreeMap::new());
    }
    rust_source_files(root, &source_directory, &mut files)?;
    let mut signals = BTreeMap::new();
    for path in files {
        let source = read_text(&path)?;
        if framework == "pinocchio" {
            for (token, id) in [
                ("account_info::AccountInfo", "pinocchio-account-info"),
                ("pubkey::Pubkey", "pinocchio-pubkey"),
            ] {
                for (offset, _) in source.match_indices(token) {
                    push_signal(
                        &mut signals,
                        id,
                        signal_evidence(root, &path, &source, offset),
                    );
                }
            }
            let compact: String = source
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            let mut search = 0;
            while let Some(relative) = compact[search..].find("fnprocess_instruction(") {
                let start = search + relative;
                let end = compact[start..]
                    .find(')')
                    .map_or(compact.len(), |length| start + length);
                let signature = &compact[start..end];
                let source_offset = source.find("fn process_instruction").unwrap_or(0);
                if signature.contains("accounts:&[AccountView]") {
                    push_signal(
                        &mut signals,
                        "pinocchio-immutable-entrypoint",
                        signal_evidence(root, &path, &source, source_offset),
                    );
                }
                if signature.contains("accounts:&mut[AccountView]") {
                    push_signal(
                        &mut signals,
                        "pinocchio-mutable-entrypoint",
                        signal_evidence(root, &path, &source, source_offset),
                    );
                }
                search = end.saturating_add(1);
                if search >= compact.len() {
                    break;
                }
            }
        } else if framework == "anchor" {
            for (offset, _) in source.match_indices("::discriminator()") {
                push_signal(
                    &mut signals,
                    "anchor-discriminator-method",
                    signal_evidence(root, &path, &source, offset),
                );
            }
            for (offset, _) in source.match_indices("CpiContext::new") {
                if first_call_argument(&source, offset)
                    .is_some_and(|argument| argument.contains(".to_account_info()"))
                {
                    push_signal(
                        &mut signals,
                        "anchor-cpi-context-account-info",
                        signal_evidence(root, &path, &source, offset),
                    );
                }
            }
        }
    }
    Ok(signals)
}

fn parse_program(
    root: &Path,
    path: &Path,
    locks: &BTreeMap<String, BTreeSet<String>>,
    workspace_requirements: &BTreeMap<String, String>,
    id: String,
    explicit: bool,
) -> Result<Option<Program>> {
    let text = read_text(path)?;
    let doc: toml::Value = toml::from_str(&text)
        .with_context(|| format!("invalid Cargo manifest {}", path.display()))?;
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
    for (key, requirement) in &mut deps {
        if requirement == "workspace" {
            if let Some(identity) = key.rsplit('/').next() {
                let package = identity
                    .split_once("=>")
                    .map_or(identity, |(_, package)| package);
                if let Some(workspace_requirement) = workspace_requirements.get(package) {
                    *requirement = workspace_requirement.clone();
                }
            }
        }
    }
    let names = cargo_dependency_names(&doc);
    let required_names = required_cargo_dependency_names(&doc);
    let cdylib = doc
        .get("lib")
        .and_then(|v| v.get("crate-type"))
        .and_then(|v| v.as_array())
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("cdylib")));
    if !explicit && !has_program_entrypoint(path, &doc)? {
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
    let source_signals = program_source_signals(root, path, framework)?;
    let mut program_evidence = vec![evidence(root, path, "/", "declared", text.as_bytes())];
    program_evidence.extend(source_signals.into_values().flatten());
    let reason = if cdylib {
        "Cargo [lib].crate-type includes cdylib"
    } else {
        "Solana program framework dependency"
    };
    let mut resolved = BTreeMap::new();
    for dep in &names {
        if let Some(v) = locks
            .get(dep)
            .filter(|v| v.len() == 1)
            .and_then(|v| v.iter().next())
        {
            resolved.insert(dep.clone(), v.clone());
        }
    }
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
fn npm_lock(root: &Path, manifest: &Path) -> Result<BTreeMap<String, String>> {
    let mut dir = manifest.parent();
    while let Some(d) = dir {
        if !d.starts_with(root) {
            break;
        }
        let p = d.join("package-lock.json");
        if exists(&p)? {
            let text = read_text(&inside(root, &p)?)?;
            let v: JsonValue = serde_json::from_str(&text).context("invalid package-lock.json")?;
            ensure!(
                matches!(
                    v.get("lockfileVersion").and_then(JsonValue::as_u64),
                    Some(2 | 3)
                ),
                "package-lock.json must use lockfileVersion 2 or 3"
            );
            let mut out = BTreeMap::new();
            if let Some(pkgs) = v.get("packages").and_then(|v| v.as_object()) {
                for (key, val) in pkgs {
                    if let Some(name) = key.strip_prefix("node_modules/") {
                        if name.contains("/node_modules/") {
                            continue;
                        }
                        let registry =
                            val.get("resolved")
                                .and_then(JsonValue::as_str)
                                .is_some_and(|source| {
                                    source.starts_with("https://registry.npmjs.org/")
                                });
                        if registry {
                            let Some(ver) = val.get("version").and_then(|v| v.as_str()) else {
                                continue;
                            };
                            out.insert(name.into(), ver.into());
                        }
                    }
                }
            }
            return Ok(out);
        }
        dir = d.parent()
    }
    Ok(BTreeMap::new())
}
fn parse_client(root: &Path, path: &Path, id: String) -> Result<Client> {
    let text = read_text(path)?;
    let p: PackageJson = serde_json::from_str(&text)
        .with_context(|| format!("invalid package manifest {}", path.display()))?;
    let mut deps = BTreeMap::new();
    for (kind, group) in [
        ("dependencies", p.dependencies),
        ("devDependencies", p.dev_dependencies),
        ("peerDependencies", p.peer_dependencies),
        ("optionalDependencies", p.optional_dependencies),
    ] {
        for (n, r) in group {
            deps.insert(format!("{kind}/{n}"), r);
        }
    }
    let declared = deps
        .keys()
        .filter_map(|key| key.split_once('/').map(|(_, name)| name.to_owned()))
        .collect::<BTreeSet<_>>();
    let mut resolved_dependencies = npm_lock(root, path)?;
    resolved_dependencies.retain(|name, _| declared.contains(name));
    Ok(Client {
        id,
        name: p.name,
        manifest: label(root, path),
        dependencies: deps,
        resolved_dependencies,
        decoder_package: None,
        required_read_versions: None,
        rpc_reads: vec![],
        evidence: vec![evidence(root, path, "/", "declared", text.as_bytes())],
    })
}
fn is_solana_client(client: &Client) -> bool {
    client.dependencies.keys().any(|key| {
        key.split_once('/').is_some_and(|(_, name)| {
            name.starts_with("@solana/")
                || name == "@coral-xyz/anchor"
                || name.starts_with("@anchor-lang/")
        })
    })
}

fn declared_client_package(client: &Client, package: &str) -> bool {
    client
        .dependencies
        .keys()
        .any(|key| key.split_once('/').is_some_and(|(_, name)| name == package))
}

fn source_files(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !EXCLUDED.contains(&name.as_ref()) {
                source_files(root, &entry.path(), out)?;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(extension, "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs") {
            out.push(inside(root, &path)?);
        }
    }
    Ok(())
}

fn call_text(source: &str, open: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in bytes
        .iter()
        .copied()
        .enumerate()
        .take(bytes.len().min(open.saturating_add(16 * 1024)))
        .skip(open)
    {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            continue;
        }
        if matches!(byte, b'\'' | b'"') || byte == 96 {
            quote = Some(byte);
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return source.get(open..=index);
                }
            }
            _ => {}
        }
    }
    None
}

fn top_level_arguments(call: &str) -> Vec<&str> {
    let inner = call
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(call);
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            continue;
        }
        if matches!(byte, b'\'' | b'"') || byte == 96 {
            quote = Some(byte);
            continue;
        }
        match byte {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < inner.len() {
        out.push(inner[start..].trim());
    }
    out
}

fn numeric_property(object: &str, property: &str) -> Option<u8> {
    let offset = object.find(property)?;
    let rest = object.get(offset + property.len()..)?;
    let colon = rest.find(':')?;
    rest.get(colon + 1..)?
        .trim_start()
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn string_property(object: &str, property: &str) -> Option<String> {
    let offset = object.find(property)?;
    let rest = object.get(offset + property.len()..)?;
    let colon = rest.find(':')?;
    let value = rest.get(colon + 1..)?.trim_start();
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') && quote != 96 {
        return None;
    }
    let tail = value.get(1..)?;
    let end = tail.find(quote as char)?;
    Some(tail[..end].to_owned())
}

fn discover_rpc_reads(root: &Path, clients: &mut [Client]) -> Result<()> {
    for client in clients {
        if !client.rpc_reads.is_empty() {
            continue;
        }
        let manifest = root.join(&client.manifest);
        let Some(directory) = manifest.parent() else {
            continue;
        };
        let mut files = Vec::new();
        source_files(root, directory, &mut files)?;
        let mut sequence = 0usize;
        for path in files {
            let source = read_text(&path)?;
            for method in ["getBlock", "getTransaction"] {
                let token = format!(".{method}(");
                let mut cursor = 0usize;
                while let Some(relative) = source[cursor..].find(&token) {
                    let method_start = cursor + relative + 1;
                    let open = method_start + method.len();
                    let Some(call) = call_text(&source, open) else {
                        cursor = open + 1;
                        continue;
                    };
                    let arguments = top_level_arguments(call);
                    let options = arguments.get(1).copied();
                    let literal = options.is_some_and(|value| value.trim_start().starts_with('{'));
                    let maximum = match options {
                        None => Some(RpcMaximum::Omitted(Omitted::Explicit)),
                        Some(value) if literal => {
                            numeric_property(value, "maxSupportedTransactionVersion")
                                .map(RpcMaximum::Version)
                                .or(Some(RpcMaximum::Omitted(Omitted::Explicit)))
                        }
                        Some(_) => None,
                    };
                    let encoding = match options {
                        Some(value) if literal => {
                            string_property(value, "encoding").or_else(|| Some("json".into()))
                        }
                        None => Some("json".into()),
                        Some(_) => None,
                    };
                    let transaction_details = if method == "getBlock" {
                        match options {
                            Some(value) if literal => string_property(value, "transactionDetails")
                                .or_else(|| Some("full".into())),
                            None => Some("full".into()),
                            Some(_) => None,
                        }
                    } else {
                        None
                    };
                    sequence += 1;
                    let line = source[..method_start]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1;
                    client.rpc_reads.push(RpcRead {
                        id: format!("source-{sequence}"),
                        method: method.into(),
                        encoding,
                        transaction_details,
                        max_supported_transaction_version: maximum,
                    });
                    client.evidence.push(Evidence {
                        path: label(root, &path),
                        pointer: format!("/source:{line}"),
                        kind: "observed".into(),
                        digest: digest(source.as_bytes()),
                    });
                    cursor = open + call.len();
                }
            }
        }
        if !client.rpc_reads.is_empty() {
            if client.required_read_versions.is_none() {
                client.required_read_versions =
                    Some(vec!["legacy".into(), "v0".into(), "v1".into()]);
            }
            if client.decoder_package.is_none() {
                client.decoder_package = [
                    "@solana/kit",
                    "@solana/web3.js",
                    "@anchor-lang/core",
                    "@coral-xyz/anchor",
                ]
                .into_iter()
                .find(|package| declared_client_package(client, package))
                .map(str::to_owned);
            }
        }
    }
    Ok(())
}
fn parse_idl(root: &Path, c: &IdlConfig, base: &Path) -> Result<IdlInput> {
    let p = inside(root, &base.join(&c.path))?;
    let text = read_text(&p)?;
    let v: JsonValue =
        serde_json::from_str(&text).with_context(|| format!("invalid IDL {}", p.display()))?;
    let schema = if let Some(s) = v.pointer("/metadata/spec").and_then(|v| v.as_str()) {
        format!("anchor-spec-{s}")
    } else if v.get("version").is_some() && v.get("instructions").is_some() {
        "anchor-legacy".into()
    } else {
        "unknown".into()
    };
    Ok(IdlInput {
        id: c.id.clone(),
        path: label(root, &p),
        client: c.client.clone(),
        reader_package: c.reader_package.clone(),
        schema,
        evidence: vec![evidence(root, &p, "/", "observed", text.as_bytes())],
    })
}
fn parse_artifact(root: &Path, c: &ArtifactConfig, base: &Path) -> Result<Artifact> {
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

fn metadata_resolutions(
    root: &Path,
    value: &JsonValue,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
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
        if package
            .get("source")
            .and_then(JsonValue::as_str)
            .is_some_and(|source| source.contains("crates.io"))
        {
            identities.insert(id.to_owned(), (name.to_owned(), version.to_owned()));
        }
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
                    if let Some((name, version)) = edge
                        .get("pkg")
                        .and_then(JsonValue::as_str)
                        .and_then(|pkg| identities.get(pkg))
                    {
                        deps.insert(name.clone(), version.clone());
                    }
                }
            }
            out.insert(manifest.clone(), deps);
        }
    }
    Ok(out)
}

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
        Some(v)
    } else {
        None
    };
    let cargo_root = cargo_workspace_root(&root)?;
    let locks = lock_versions(&cargo_root)?;
    let inherited_requirements = workspace_requirements(&cargo_root)?;
    let mut clients = Vec::new();
    let mut client_paths = BTreeSet::new();
    let mut client_ids = BTreeSet::new();
    for (i, c) in config.clients.iter().enumerate() {
        ensure!(
            identifier(&c.id) && client_ids.insert(c.id.clone()),
            "client ids must be valid and unique"
        );
        let p = inside(&root, &base.join(&c.manifest))?;
        let mut client = parse_client(&root, &p, c.id.clone())?;
        client_paths.insert(client.manifest.clone());
        if let Some(d) = &c.decoder_package {
            ensure!(
                client
                    .dependencies
                    .keys()
                    .any(|k| k.split_once('/').is_some_and(|x| x.1 == d)),
                "decoder_package {d} is not declared in {}",
                client.manifest
            );
            client.decoder_package = Some(d.clone());
        }
        if let Some(v) = &c.required_read_versions {
            ensure!(
                v.iter().all(|x| tx_version(x)),
                "invalid required_read_versions"
            );
            ensure!(
                v.iter().collect::<BTreeSet<_>>().len() == v.len(),
                "duplicate required_read_versions for {}",
                c.id
            );
            let mut versions = v.clone();
            versions.sort();
            client.required_read_versions = Some(versions);
        }
        ensure!(
            !(client
                .required_read_versions
                .as_ref()
                .is_some_and(Vec::is_empty)
                && !c.rpc_reads.is_empty()),
            "client {} cannot declare required_read_versions = [] and configure RPC reads",
            c.id
        );
        let mut ids = BTreeSet::new();
        for (ri, r) in c.rpc_reads.iter().enumerate() {
            ensure!(
                matches!(r.method.as_str(), "getBlock" | "getTransaction"),
                "unsupported RPC method: {}",
                r.method
            );
            if let Some(encoding) = &r.encoding {
                ensure!(!encoding.trim().is_empty(), "encoding must not be empty");
            }
            if let Some(details) = &r.transaction_details {
                ensure!(
                    r.method == "getBlock",
                    "transaction_details is only valid for getBlock"
                );
                ensure!(
                    matches!(
                        details.as_str(),
                        "full" | "accounts" | "signatures" | "none"
                    ),
                    "invalid transaction_details: {details}"
                );
            }
            let id = r.id.clone().unwrap_or_else(|| format!("read-{}", ri + 1));
            ensure!(
                identifier(&id) && ids.insert(id.clone()),
                "RPC read ids must be valid and unique"
            );
            client.rpc_reads.push(RpcRead {
                id,
                method: r.method.clone(),
                encoding: r.encoding.clone(),
                transaction_details: r.transaction_details.clone(),
                max_supported_transaction_version: r.max_supported_transaction_version.clone(),
            });
        }
        if let Some(ev) = &config_ev {
            let mut ev = ev.clone();
            ev.pointer = format!("clients[{i}]");
            client.evidence.push(ev);
        }
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
        let client = parse_client(&root, &p, id.clone())?;
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
        let mut prog = parse_program(
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
        prog.anchor_cli_version = c.anchor_cli_version.clone();
        prog.sbf_rust_version = c.sbf_rust_version.clone();
        prog.sbpf_arch = c.sbpf_arch.clone();
        prog.platform_tools_version = c.platform_tools_version.clone();
        prog.cargo_build_sbf_version = c.cargo_build_sbf_version.clone();
        if let Some(ev) = &config_ev {
            let mut ev = ev.clone();
            ev.pointer = format!("programs[{i}]");
            prog.evidence.push(ev);
        }
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
        if let Some(prog) = parse_program(
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
    if let Some(metadata) = &metadata {
        let resolutions = metadata_resolutions(&root, metadata)?;
        for program in &mut programs {
            if let Some(dependencies) = resolutions.get(&program.manifest) {
                program.resolved_dependencies = dependencies.clone();
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
            if program.anchor_cli_version.is_none() {
                program.anchor_cli_version = anchor_version.clone();
            }
            program.evidence.push(evidence(
                &root,
                &path,
                "/toolchain",
                "declared",
                text.as_bytes(),
            ));
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
            ["SC001", "SC003", "SC102", "SC103", "SC104", "SC200", "SC201", "SC302", "SC400"]
                .contains(&s.rule.as_str()),
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
            let executable = Some(path.to_string_lossy().into_owned());
            let mut cmd = Command::new(&path);
            cmd.arg("--version")
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped());
            match cmd.spawn() {
                Err(e) => ToolObservation {
                    tool: tool.into(),
                    version: None,
                    executable: executable.clone(),
                    status: format!("unavailable: {e}"),
                },
                Ok(mut child) => {
                    let start = Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                let output = child.wait_with_output();
                                break match output {
                                    Ok(o) => {
                                        let text = String::from_utf8_lossy(&o.stdout);
                                        let ver = text
                                            .split_whitespace()
                                            .find(|s| {
                                                s.chars().next().is_some_and(|c| c.is_ascii_digit())
                                            })
                                            .map(|s| s.trim_start_matches('v').to_owned());
                                        ToolObservation {
                                            tool: tool.into(),
                                            version: ver,
                                            executable: executable.clone(),
                                            status: "observed".into(),
                                        }
                                    }
                                    Err(e) => ToolObservation {
                                        tool: tool.into(),
                                        version: None,
                                        executable: executable.clone(),
                                        status: format!("probe failed: {e}"),
                                    },
                                };
                            }
                            Ok(None) if start.elapsed() < Duration::from_secs(3) => {
                                thread::sleep(Duration::from_millis(20))
                            }
                            Ok(None) => {
                                let _ = child.kill();
                                let _ = child.wait();
                                break ToolObservation {
                                    tool: tool.into(),
                                    version: None,
                                    executable: executable.clone(),
                                    status: "timed out".into(),
                                };
                            }
                            Err(e) => {
                                break ToolObservation {
                                    tool: tool.into(),
                                    version: None,
                                    executable: executable.clone(),
                                    status: format!("probe failed: {e}"),
                                }
                            }
                        }
                    }
                }
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct BuildObservation {
    pub success: bool,
    pub command: String,
    pub manifest: String,
    pub output: String,
}

/// Run the normal SBF build for one detected program. This is only called from
/// the explicit `--build` workflow; project discovery never executes code.
pub fn build_program(root: &Path, program: &Program) -> Result<BuildObservation> {
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
    command.stdin(Stdio::null());
    let output = command
        .output()
        .with_context(|| format!("failed to start `{display}`"))?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<_> = combined.lines().collect();
    let start = lines.len().saturating_sub(40);
    Ok(BuildObservation {
        success: output.status.success(),
        command: display,
        manifest: program.manifest.clone(),
        output: lines[start..].join("\n"),
    })
}
