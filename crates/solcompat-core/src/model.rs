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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRead {
    pub id: String,
    pub method: String,
    pub encoding: Option<String>,
    pub transaction_details: Option<String>,
    pub max_supported_transaction_version: Option<RpcMaximum>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub path: String,
    pub pointer: String,
    pub kind: String,
    pub digest: String,
}

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

impl Report {
    pub fn finish(&mut self) {
        self.results
            .sort_by(|a, b| (&a.subject, &a.rule_id).cmp(&(&b.subject, &b.rule_id)));
        self.counts = Counts::default();
        self.exit_code = 0;
        for result in &self.results {
            if result.suppression.is_some() {
                self.counts.suppressed += 1;
                continue;
            }
            match result.outcome {
                Outcome::Pass => self.counts.passed += 1,
                Outcome::Unknown => {
                    self.counts.unknown += 1;
                    if self.policy.deny_unknown {
                        self.exit_code = 1;
                    }
                }
                Outcome::NotApplicable => self.counts.not_applicable += 1,
                Outcome::Skipped => self.counts.skipped += 1,
                Outcome::Finding => match result.severity {
                    Some(Severity::Error) => {
                        self.counts.errors += 1;
                        self.exit_code = 1;
                    }
                    Some(Severity::Warning) => {
                        self.counts.warnings += 1;
                        if self.policy.deny_warnings {
                            self.exit_code = 1;
                        }
                    }
                    _ => self.counts.info += 1,
                },
            }
        }
    }
}
