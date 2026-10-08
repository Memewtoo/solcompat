use super::*;
use crate::{Evidence, Outcome, Program, SourceSignal};
use std::collections::BTreeMap;

fn project(current: Option<&str>, signals: &[SourceSignal]) -> Project {
    Project {
        name: "memory-only".into(),
        clients: vec![],
        artifacts: vec![],
        idls: vec![],
        tools: vec![],
        programs: vec![Program {
            id: "program".into(),
            name: "program".into(),
            manifest: "nonexistent/Cargo.toml".into(),
            framework: "anchor".into(),
            candidate_reason: "test".into(),
            rust_version: None,
            dependencies: BTreeMap::from([("dependencies/solana-program".into(), "2".into())]),
            resolved_dependencies: current
                .map(|value| BTreeMap::from([("anchor-lang".into(), value.into())]))
                .unwrap_or_default(),
            anchor_cli_version: None,
            sbf_rust_version: None,
            sbpf_arch: None,
            platform_tools_version: None,
            cargo_build_sbf_version: None,
            evidence: signals
                .iter()
                .map(|signal| Evidence {
                    path: "nonexistent/source.rs".into(),
                    pointer: "/source:1".into(),
                    kind: signal.evidence_kind().into(),
                    digest: "test".into(),
                })
                .collect(),
        }],
    }
}
fn ids(report: &Report) -> Vec<&str> {
    report
        .results
        .iter()
        .map(|item| item.rule_id.as_str())
        .collect()
}

#[test]
fn pure_upgrade_api_reports_only_crossed_anchor_boundaries() {
    let data = Dataset::bundled().unwrap();
    let source = project(
        Some("0.30.1"),
        &[
            SourceSignal::AnchorDiscriminatorMethod,
            SourceSignal::AnchorCpiContextAccountInfo,
        ],
    );
    let report = upgrade_report(source.clone(), &data, "0.31.0", "4.3.0").unwrap();
    assert_eq!(ids(&report), vec!["UP101", "UP106"]);
    assert_eq!(report.exit_code, 1);
    let report = upgrade_report(source, &data, "1.2.0", "4.4.0").unwrap();
    assert_eq!(
        ids(&report),
        vec!["UP200", "UP101", "UP102", "UP103", "UP106", "UP107", "UP105"]
    );
    for result in report
        .results
        .iter()
        .filter(|item| matches!(item.rule_id.as_str(), "UP106" | "UP107"))
    {
        assert_eq!(result.evidence.len(), 1);
        assert_eq!(result.evidence[0].path, "nonexistent/source.rs");
        assert_eq!(
            result.remediation.as_ref().unwrap().location,
            "nonexistent/source.rs"
        );
    }
    let report = upgrade_report(project(Some("1.2.0"), &[]), &data, "1.2.0", "4.3.0").unwrap();
    assert_eq!(ids(&report), vec!["UP100"]);
    assert_eq!(report.results[0].outcome, Outcome::Pass);
}

#[test]
fn unresolved_and_invalid_targets_do_not_claim_source_migrations() {
    let data = Dataset::bundled().unwrap();
    let source = project(None, &[SourceSignal::AnchorDiscriminatorMethod]);
    let report = upgrade_report(source.clone(), &data, "1.2.0", "4.3.0").unwrap();
    assert_eq!(ids(&report), vec!["UP100"]);
    assert_eq!(report.results[0].outcome, Outcome::Unknown);
    assert_eq!(report.exit_code, 0);
    assert!(upgrade_report(source.clone(), &data, "0.30.1", "4.3.0").is_err());
    assert!(upgrade_report(source, &data, "1.2.0", "invalid")
        .unwrap_err()
        .to_string()
        .contains("invalid Agave target version"));
}
