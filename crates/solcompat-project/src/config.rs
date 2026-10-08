//! Configuration input shapes and validation helpers; binding validation lives in collect.

use serde::Deserialize;
use solcompat_core::{RpcMaximum, Suppression};
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisConfig {
    pub(crate) target: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) analysis: AnalysisConfig,
    #[serde(default)]
    pub(crate) clients: Vec<ClientConfig>,
    #[serde(default)]
    pub(crate) programs: Vec<ProgramConfig>,
    #[serde(default)]
    pub(crate) idls: Vec<IdlConfig>,
    #[serde(default)]
    pub(crate) artifacts: Vec<ArtifactConfig>,
    #[serde(default)]
    pub(crate) suppressions: Vec<Suppression>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientConfig {
    pub(crate) id: String,
    pub(crate) manifest: PathBuf,
    pub(crate) decoder_package: Option<String>,
    pub(crate) required_read_versions: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) rpc_reads: Vec<ReadConfig>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProgramConfig {
    pub(crate) id: String,
    pub(crate) manifest: PathBuf,
    pub(crate) anchor_cli_version: Option<String>,
    pub(crate) sbf_rust_version: Option<String>,
    pub(crate) sbpf_arch: Option<String>,
    pub(crate) platform_tools_version: Option<String>,
    pub(crate) cargo_build_sbf_version: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdlConfig {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) client: String,
    pub(crate) reader_package: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactConfig {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) program: Option<String>,
    #[serde(default = "default_loader")]
    pub(crate) loader: String,
    #[serde(default = "default_operation")]
    pub(crate) operation: String,
}
pub(crate) fn default_loader() -> String {
    "loader-v3".into()
}
pub(crate) fn default_operation() -> String {
    "upgrade".into()
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadConfig {
    pub(crate) id: Option<String>,
    pub(crate) method: String,
    pub(crate) encoding: Option<String>,
    pub(crate) transaction_details: Option<String>,
    pub(crate) max_supported_transaction_version: Option<RpcMaximum>,
}
pub(crate) fn identifier(v: &str) -> bool {
    !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
pub(crate) fn tx_version(v: &str) -> bool {
    v == "legacy"
        || v.strip_prefix('v')
            .and_then(|x| x.parse::<u8>().ok())
            .is_some_and(|n| n <= 127 && v == format!("v{n}"))
}
