use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RpcMaximum {
    Version(u8),
    Omitted(Omitted),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Omitted {
    #[serde(rename = "omitted")]
    Explicit,
}

/// One configured or textually observed RPC request. A missing maximum
/// means unresolved evidence; an explicit omitted maximum means an observed or
/// asserted omission. Neither establishes decoder capability.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRead {
    pub id: String,
    pub method: String,
    pub encoding: Option<String>,
    pub transaction_details: Option<String>,
    pub max_supported_transaction_version: Option<RpcMaximum>,
}

/// Source reference and byte digest. Some kind strings identify recognized
/// framework syntax; evidence is not a full per-field provenance graph.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub path: String,
    pub pointer: String,
    pub kind: String,
    pub digest: String,
}

/// Collected npm client inputs. Constructed by `collect_client` in
/// solcompat-project/src/client.rs, enriched by collect.rs and source/rpc.rs.
/// Declared requirements and exact resolved versions have separate fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub id: String,
    pub name: Option<String>,
    pub manifest: String,
    /// Declared requirements only; never resolved or installed versions.
    pub dependencies: BTreeMap<String, String>,
    /// Exact installed/locked package versions when a lockfile or metadata import proves them.
    pub resolved_dependencies: BTreeMap<String, String>,
    pub decoder_package: Option<String>,
    pub required_read_versions: Option<Vec<String>>,
    pub rpc_reads: Vec<RpcRead>,
    pub evidence: Vec<Evidence>,
}

/// Collected program inputs consumed by the rule engine.
///
/// The constructor is `collect_program` in solcompat-project/src/program.rs.
/// That function creates the struct directly; no model-owned builder is needed.
/// collect.rs applies configuration, metadata, and Anchor.toml enrichment;
/// project/tools.rs fills absent tool selections from explicitly requested probes.
/// See docs/ARCHITECTURE.md for the field-source and precedence tables.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub id: String,
    pub name: String,
    pub manifest: String,
    pub framework: String,
    pub candidate_reason: String,
    pub rust_version: Option<String>,
    pub dependencies: BTreeMap<String, String>,
    pub resolved_dependencies: BTreeMap<String, String>,
    pub anchor_cli_version: Option<String>,
    pub sbf_rust_version: Option<String>,
    pub sbpf_arch: Option<String>,
    pub platform_tools_version: Option<String>,
    pub cargo_build_sbf_version: Option<String>,
    pub evidence: Vec<Evidence>,
}

/// An explicitly bound IDL and its reader relationship, parsed in
/// solcompat-project/src/idl.rs. IDLs are not automatically assigned to clients.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdlInput {
    pub id: String,
    pub path: String,
    pub client: String,
    pub reader_package: String,
    pub schema: String,
    pub evidence: Vec<Evidence>,
}

/// An existing ELF header inspected in solcompat-project/src/artifact.rs.
/// Its byte identity does not prove that the current source produced it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub path: String,
    pub program: Option<String>,
    pub loader: String,
    pub operation: String,
    pub elf_machine: u16,
    pub elf_flags: u32,
    pub sbpf_version: Option<String>,
    pub evidence: Vec<Evidence>,
}

/// An opt-in installed executable observation from solcompat-project::probe_tools.
/// Selected tool versions in configuration are distinct from these observations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolObservation {
    pub tool: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPolicy {
    pub loader: String,
    pub operations: Vec<String>,
    pub allowed_sbpf_versions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisTarget {
    pub id: String,
    #[serde(default)]
    pub conditional: bool,
    #[serde(default)]
    pub artifact_policies: Vec<ArtifactPolicy>,
    #[serde(skip)]
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Project {
    pub name: String,
    pub clients: Vec<Client>,
    pub programs: Vec<Program>,
    pub idls: Vec<IdlInput>,
    pub artifacts: Vec<Artifact>,
    pub tools: Vec<ToolObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Suppression {
    pub rule: String,
    pub subject: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Finding,
    Unknown,
    NotApplicable,
    Skipped,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct Remediation {
    pub summary: String,
    pub location: String,
    pub steps: Vec<String>,
    pub example: Option<String>,
    pub verification: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckResult {
    pub rule_id: String,
    pub rule_name: String,
    pub rule_revision: u32,
    pub subject: String,
    pub operation: String,
    pub conditional: bool,
    pub outcome: Outcome,
    pub severity: Option<Severity>,
    pub title: String,
    pub summary: String,
    pub explanation: String,
    pub evidence: Vec<Evidence>,
    pub sources: Vec<String>,
    pub remediation: Option<Remediation>,
    pub suppression: Option<Suppression>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Policy {
    pub deny_warnings: bool,
    pub deny_unknown: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub passed: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    pub unknown: usize,
    pub not_applicable: usize,
    pub skipped: usize,
    pub suppressed: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct DatasetIdentity {
    pub revision: String,
    pub digest: String,
}

/// Serialized inventory or evaluation report. CLI orchestration assembles
/// inspect reports, rules::check assembles current checks, and upgrade::upgrade_report
/// assembles upgrade advice. [`Report::finish`] derives counts and exits.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub command: String,
    pub analysis_mode: String,
    pub target: String,
    pub target_digest: Option<String>,
    pub dataset: DatasetIdentity,
    pub project: Project,
    pub results: Vec<CheckResult>,
    pub counts: Counts,
    pub policy: Policy,
    pub exit_code: u8,
}
