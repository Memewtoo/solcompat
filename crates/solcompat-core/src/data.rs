use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const BUNDLED: &str = include_str!("../compatibility/records/rpc-read-contracts.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRecord {
    pub id: String,
    pub revision: u32,
    pub rule_id: String,
    pub rule_name: String,
    pub verified_on: String,
    pub evidence_kind: String,
    pub sources: Vec<String>,
    pub reviewed_versions: Vec<String>,
    pub reviewed_encodings: Vec<String>,
    pub methods: Vec<String>,
    pub body_free_block_details: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    revision: String,
    engine_schema: u32,
    records: Vec<RpcRecord>,
    #[serde(default)]
    rules: Vec<RuleRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRecord {
    pub rule_id: String,
    pub rule_name: String,
    pub revision: u32,
    pub verified_on: String,
    pub sources: Vec<String>,
    pub notes: String,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub revision: String,
    pub digest: String,
    record: RpcRecord,
    rules: Vec<RuleRecord>,
}

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

fn unique_subset(values: &[String], supported: &[&str], field: &str) -> Result<()> {
    let unique: BTreeSet<_> = values.iter().collect();
    ensure!(!values.is_empty(), "dataset {field} must not be empty");
    ensure!(unique.len() == values.len(), "duplicate dataset {field}");
    ensure!(
        values
            .iter()
            .all(|value| supported.contains(&value.as_str())),
        "unsupported dataset {field}"
    );
    Ok(())
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        value[0..4].parse::<u32>(),
        value[5..7].parse::<u32>(),
        value[8..10].parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

impl Dataset {
    pub fn bundled() -> Result<Self> {
        Self::parse(BUNDLED)
    }

    pub fn parse(input: &str) -> Result<Self> {
        let document: Document =
            serde_json::from_str(input).context("invalid compatibility dataset JSON")?;
        ensure!(
            document.schema_version == 1,
            "unsupported dataset schema_version"
        );
        ensure!(
            document.engine_schema == 1,
            "dataset requires an unsupported engine schema"
        );
        ensure!(
            !document.revision.trim().is_empty(),
            "dataset revision is required"
        );
        ensure!(
            document.records.len() == 1,
            "exactly one SC201 record is required; overlapping records are not supported"
        );
        let record = document.records.into_iter().next().expect("length checked");
        ensure!(
            record.rule_id == "SC201" && record.id == "rpc-read-version-acceptance",
            "unsupported compatibility record"
        );
        ensure!(
            record.revision > 0 && !record.rule_name.trim().is_empty(),
            "record name and positive revision are required"
        );
        ensure!(
            record.evidence_kind == "upstream_documentation",
            "unsupported evidence kind"
        );
        let date = record.verified_on.as_bytes();
        ensure!(
            date.len() == 10
                && date[4] == b'-'
                && date[7] == b'-'
                && date
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()),
            "verified_on must be YYYY-MM-DD"
        );
        let year: u32 = record.verified_on[0..4].parse()?;
        let month: u32 = record.verified_on[5..7].parse()?;
        let day: u32 = record.verified_on[8..10].parse()?;
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        ensure!(
            year > 0 && day > 0 && day <= days,
            "verified_on must be a valid calendar date"
        );
        ensure!(
            !record.sources.is_empty()
                && record.sources.iter().all(|url| url.starts_with("https://")
                    && url.len() > 8
                    && !url.chars().any(char::is_whitespace)),
            "record requires HTTPS source references"
        );
        ensure!(
            !record.notes.trim().is_empty(),
            "record requires scope notes"
        );
        unique_subset(
            &record.reviewed_versions,
            &["legacy", "v0", "v1"],
            "reviewed_versions",
        )?;
        unique_subset(
            &record.reviewed_encodings,
            &["json", "jsonParsed", "base64", "base58"],
            "reviewed_encodings",
        )?;
        unique_subset(&record.methods, &["getBlock", "getTransaction"], "methods")?;
        unique_subset(
            &record.body_free_block_details,
            &["none", "signatures"],
            "body_free_block_details",
        )?;
        let expected = [
            "SC001", "SC003", "SC102", "SC103", "SC104", "SC200", "SC302", "SC400",
        ];
        ensure!(
            document.rules.len() == expected.len(),
            "dataset requires all eight compatibility rule records"
        );
        let mut seen = BTreeSet::new();
        for rule in &document.rules {
            ensure!(
                expected.contains(&rule.rule_id.as_str()),
                "unsupported rule record {}",
                rule.rule_id
            );
            ensure!(
                seen.insert(rule.rule_id.clone()),
                "duplicate rule record {}",
                rule.rule_id
            );
            ensure!(
                rule.revision > 0 && !rule.rule_name.trim().is_empty(),
                "rule metadata is incomplete"
            );
            ensure!(
                valid_date(&rule.verified_on),
                "verified_on must be a valid calendar date"
            );
            ensure!(
                !rule.sources.is_empty()
                    && rule
                        .sources
                        .iter()
                        .all(|url| url.starts_with("https://")
                            && !url.chars().any(char::is_whitespace)),
                "record requires HTTPS source references"
            );
            ensure!(!rule.notes.trim().is_empty(), "record requires scope notes");
        }
        Ok(Self {
            revision: document.revision,
            digest: crate::digest(input.as_bytes()),
            record,
            rules: document.rules,
        })
    }

    pub fn rpc_record(&self) -> &RpcRecord {
        &self.record
    }

    pub fn rule(&self, id: &str) -> &RuleRecord {
        self.rules
            .iter()
            .find(|rule| rule.rule_id == id)
            .expect("validated bundled rule")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_data_never_becomes_empty_success() {
        let original: serde_json::Value = serde_json::from_str(BUNDLED).unwrap();
        for field in ["schema_version", "engine_schema"] {
            let mut value = original.clone();
            value[field] = 99.into();
            assert!(Dataset::parse(&value.to_string()).is_err());
        }
        let mut duplicate = original.clone();
        let record = duplicate["records"][0].clone();
        duplicate["records"].as_array_mut().unwrap().push(record);
        assert!(Dataset::parse(&duplicate.to_string()).is_err());
        for field in ["sources", "reviewed_versions", "methods"] {
            let mut value = original.clone();
            value["records"][0][field] = serde_json::json!([]);
            assert!(Dataset::parse(&value.to_string()).is_err());
        }
        let mut future = original;
        future["records"][0]["reviewed_versions"] = serde_json::json!(["v2"]);
        assert!(Dataset::parse(&future.to_string()).is_err());
    }

    #[test]
    fn invalid_review_dates_are_rejected() {
        let mut document: serde_json::Value = serde_json::from_str(BUNDLED).unwrap();
        for date in [
            "2026-02-29",
            "2026-99-01",
            "2026-01-00",
            "0000-01-01",
            "not-a-date",
        ] {
            document["records"][0]["verified_on"] = date.into();
            assert!(Dataset::parse(&document.to_string()).is_err(), "{date}");
        }
        document["records"][0]["verified_on"] = "2024-02-29".into();
        assert!(Dataset::parse(&document.to_string()).is_ok());
    }
}
