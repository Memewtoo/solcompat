//! Named and explicit artifact target policy; separate from compatibility data.
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetDocument {
    schema_version: u32,
    id: String,
    #[serde(default)]
    conditional: bool,
    #[serde(default)]
    artifact_policies: Vec<crate::ArtifactPolicy>,
}

impl crate::AnalysisTarget {
    pub fn named(id: &str) -> Result<Self> {
        ensure!(
            matches!(
                id,
                "local" | "mainnet-beta-2026-09-26" | "future-sbpfv3-deployment"
            ),
            "unsupported target: {id}"
        );
        let artifact_policies = if id == "future-sbpfv3-deployment" {
            vec![crate::ArtifactPolicy {
                loader: "loader-v3".into(),
                operations: vec!["deploy".into(), "finalize".into(), "upgrade".into()],
                allowed_sbpf_versions: vec!["v3".into()],
            }]
        } else {
            vec![]
        };
        Ok(Self {
            id: id.into(),
            conditional: id == "future-sbpfv3-deployment",
            artifact_policies,
            digest: None,
        })
    }

    pub fn parse(input: &str) -> Result<Self> {
        let document: TargetDocument =
            serde_json::from_str(input).context("invalid target JSON")?;
        ensure!(
            document.schema_version == 1,
            "unsupported target schema_version"
        );
        ensure!(!document.id.trim().is_empty(), "target id is required");
        let mut pairs = BTreeSet::new();
        for policy in &document.artifact_policies {
            ensure!(
                !policy.loader.trim().is_empty(),
                "target loader is required"
            );
            ensure!(
                !policy.operations.is_empty()
                    && policy.operations.iter().all(|operation| matches!(
                        operation.as_str(),
                        "deploy" | "upgrade" | "finalize" | "execute"
                    )),
                "target has unsupported artifact operation"
            );
            ensure!(
                !policy.allowed_sbpf_versions.is_empty()
                    && policy
                        .allowed_sbpf_versions
                        .iter()
                        .all(|version| matches!(version.as_str(), "v0" | "v1" | "v2" | "v3")),
                "target has unsupported sBPF version"
            );
            for operation in &policy.operations {
                ensure!(
                    pairs.insert((policy.loader.clone(), operation.clone())),
                    "overlapping target artifact policy"
                );
            }
        }
        Ok(Self {
            id: document.id,
            conditional: document.conditional,
            artifact_policies: document.artifact_policies,
            digest: Some(crate::digest(input.as_bytes())),
        })
    }
}
