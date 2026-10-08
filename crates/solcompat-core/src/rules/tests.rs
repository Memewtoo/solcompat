use super::*;
use crate::{CheckResult, Client, IdlInput, Omitted, Outcome, RpcMaximum, RpcRead};
use std::collections::BTreeMap;

fn client(maximum: Option<RpcMaximum>) -> Client {
    Client {
        id: "indexer".into(),
        name: None,
        manifest: "package.json".into(),
        dependencies: BTreeMap::new(),
        resolved_dependencies: BTreeMap::new(),
        decoder_package: None,
        required_read_versions: Some(vec!["legacy".into(), "v0".into(), "v1".into()]),
        rpc_reads: vec![RpcRead {
            id: "blocks".into(),
            method: "getBlock".into(),
            encoding: Some("json".into()),
            transaction_details: Some("full".into()),
            max_supported_transaction_version: maximum,
        }],
        evidence: vec![],
    }
}

fn run(client: &Client) -> CheckResult {
    evaluate(
        client,
        client.rpc_reads.first().expect("test read"),
        &Dataset::bundled().unwrap(),
    )
}

#[test]
fn acceptance_boundaries_and_omission_are_distinct() {
    for (maximum, expected) in [
        (Some(RpcMaximum::Version(0)), Outcome::Finding),
        (Some(RpcMaximum::Version(1)), Outcome::Pass),
        (
            Some(RpcMaximum::Omitted(Omitted::Explicit)),
            Outcome::Finding,
        ),
        (None, Outcome::Unknown),
        (Some(RpcMaximum::Version(2)), Outcome::Unknown),
    ] {
        assert_eq!(run(&client(maximum)).outcome, expected);
    }
    let mut legacy = client(Some(RpcMaximum::Omitted(Omitted::Explicit)));
    legacy.required_read_versions = Some(vec!["legacy".into()]);
    assert_eq!(run(&legacy).outcome, Outcome::Pass);
    legacy.required_read_versions = Some(vec!["v0".into()]);
    assert_eq!(run(&legacy).outcome, Outcome::Finding);
    legacy.rpc_reads[0].max_supported_transaction_version = Some(RpcMaximum::Version(0));
    assert_eq!(run(&legacy).outcome, Outcome::Pass);
}

#[test]
fn response_modes_and_unknowns_do_not_create_false_failures() {
    let mut value = client(None);
    for details in ["none", "signatures"] {
        value.rpc_reads[0].transaction_details = Some(details.into());
        assert_eq!(run(&value).outcome, Outcome::NotApplicable);
    }
    value.rpc_reads[0].transaction_details = Some("accounts".into());
    assert_eq!(run(&value).outcome, Outcome::Unknown);
    value.rpc_reads[0].method = "getTransaction".into();
    value.rpc_reads[0].transaction_details = None;
    value.rpc_reads[0].max_supported_transaction_version = Some(RpcMaximum::Version(1));
    assert_eq!(run(&value).outcome, Outcome::Pass);
    value.required_read_versions = Some(vec!["v2".into()]);
    assert_eq!(run(&value).outcome, Outcome::Unknown);
    value.required_read_versions = None;
    assert_eq!(run(&value).outcome, Outcome::Unknown);
    value.required_read_versions = Some(vec![]);
    assert_eq!(run(&value).outcome, Outcome::NotApplicable);
}

fn program() -> Program {
    Program {
        id: "vault".into(),
        name: "vault".into(),
        manifest: "programs/vault/Cargo.toml".into(),
        framework: "anchor".into(),
        candidate_reason: "test".into(),
        rust_version: Some("1.79".into()),
        dependencies: BTreeMap::from([("dependencies/anchor-lang".into(), "0.31.1".into())]),
        resolved_dependencies: BTreeMap::from([
            ("anchor-lang".into(), "0.31.1".into()),
            ("anchor-spl".into(), "0.31.1".into()),
        ]),
        anchor_cli_version: Some("0.31.1".into()),
        sbf_rust_version: Some("1.79.0".into()),
        sbpf_arch: Some("v3".into()),
        platform_tools_version: Some("1.53.0".into()),
        cargo_build_sbf_version: Some("4.2.0".into()),
        evidence: vec![],
    }
}

fn outcome(results: &[CheckResult], id: &str) -> Outcome {
    results
        .iter()
        .find(|result| result.rule_id == id)
        .unwrap()
        .outcome
}

#[test]
fn program_rules_preserve_pass_finding_unknown_and_applicability() {
    let data = Dataset::bundled().unwrap();
    let baseline = program();
    let results = program_results(&baseline, &data);
    for id in ["SC001", "SC003", "SC103", "SC302"] {
        assert_eq!(outcome(&results, id), Outcome::Pass, "{id}");
    }

    let mut changed = baseline.clone();
    changed.sbf_rust_version = Some("1.78.0".into());
    changed.anchor_cli_version = Some("0.30.1".into());
    changed.platform_tools_version = Some("1.52.0".into());
    changed
        .resolved_dependencies
        .insert("anchor-spl".into(), "0.30.1".into());
    let results = program_results(&changed, &data);
    for id in ["SC001", "SC003", "SC103", "SC302"] {
        assert_eq!(outcome(&results, id), Outcome::Finding, "{id}");
    }
    let sc003 = results
        .iter()
        .find(|result| result.rule_id == "SC003")
        .unwrap();
    assert!(sc003.summary.contains("Cargo accepted anchor-lang 0.31.1"));
    assert!(sc003.summary.contains("requirement 0.31.1"));
    assert!(sc003.explanation.contains("dependency-range compatibility"));

    let mut unresolved_anchor = baseline.clone();
    unresolved_anchor
        .resolved_dependencies
        .remove("anchor-lang");
    let results = program_results(&unresolved_anchor, &data);
    let sc003 = results
        .iter()
        .find(|result| result.rule_id == "SC003")
        .unwrap();
    assert_eq!(sc003.outcome, Outcome::Unknown);
    assert_eq!(sc003.title, "Resolved anchor-lang version is unavailable");
    assert!(sc003.summary.contains("Anchor CLI 0.31.1 is selected"));
    assert!(sc003.summary.contains("requirement 0.31.1 is declared"));
    assert!(!sc003
        .remediation
        .as_ref()
        .unwrap()
        .summary
        .contains("anchor_cli_version"));
    assert!(sc003.remediation.as_ref().unwrap().steps[0].contains("anchor build"));

    let mut missing = baseline.clone();
    missing.sbf_rust_version = None;
    missing.anchor_cli_version = None;
    missing.platform_tools_version = None;
    missing.resolved_dependencies.remove("anchor-lang");
    let results = program_results(&missing, &data);
    for id in ["SC001", "SC003", "SC103"] {
        assert_eq!(outcome(&results, id), Outcome::Unknown, "{id}");
    }

    let mut native = baseline;
    native.framework = "native".into();
    native.rust_version = None;
    native.sbpf_arch = None;
    let results = program_results(&native, &data);
    for id in ["SC001", "SC003", "SC103", "SC302"] {
        assert_eq!(outcome(&results, id), Outcome::NotApplicable, "{id}");
    }
}

#[test]
fn decoder_and_idl_rules_require_exact_bound_evidence() {
    let data = Dataset::bundled().unwrap();
    let mut value = client(Some(RpcMaximum::Version(1)));
    value.decoder_package = Some("@solana/web3.js".into());
    value
        .resolved_dependencies
        .insert("@solana/web3.js".into(), "1.99.0".into());
    assert_eq!(decoder_result(&value, &data).outcome, Outcome::Pass);
    value
        .resolved_dependencies
        .insert("@solana/web3.js".into(), "1.98.4".into());
    assert_eq!(decoder_result(&value, &data).outcome, Outcome::Finding);
    value.resolved_dependencies.clear();
    assert_eq!(decoder_result(&value, &data).outcome, Outcome::Unknown);
    value.required_read_versions = Some(vec![]);
    assert_eq!(
        decoder_result(&value, &data).outcome,
        Outcome::NotApplicable
    );

    // An RPC maximum that accepts V1 independently places V1 decoding in
    // scope; the compatibility contract does not need to repeat it.
    value.required_read_versions = None;
    value
        .resolved_dependencies
        .insert("@solana/kit".into(), "8.0.0".into());
    value.decoder_package = Some("@solana/kit".into());
    assert_eq!(decoder_result(&value, &data).outcome, Outcome::Pass);

    value
        .resolved_dependencies
        .insert("@coral-xyz/anchor".into(), "0.30.1".into());
    let idl = IdlInput {
        id: "vault-idl".into(),
        path: "idl/vault.json".into(),
        client: "indexer".into(),
        reader_package: "@coral-xyz/anchor".into(),
        schema: "anchor-spec-0.1.0".into(),
        evidence: vec![],
    };
    let mut project = Project {
        name: "test".into(),
        clients: vec![value.clone()],
        programs: vec![],
        idls: vec![],
        artifacts: vec![],
        tools: vec![],
    };
    assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Pass);
    project.clients[0]
        .resolved_dependencies
        .insert("@coral-xyz/anchor".into(), "0.29.0".into());
    assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Finding);
    project.clients[0].resolved_dependencies.clear();
    assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Unknown);
}

#[test]
fn alignment_uses_the_normal_requirement_and_syntax_results_keep_resolution_evidence() {
    let mut value = program();
    value
        .dependencies
        .insert("buildDependencies/anchor-lang".into(), "0.30".into());
    assert_eq!(
        crate::evaluation::declared_requirement(&value, "anchor-lang"),
        Some("0.31.1")
    );
    value.framework = "pinocchio".into();
    value
        .resolved_dependencies
        .insert("pinocchio".into(), "0.11.2".into());
    value.evidence = vec![
        crate::Evidence {
            path: "Cargo.lock".into(),
            pointer: "/package".into(),
            kind: "resolution".into(),
            digest: "test".into(),
        },
        crate::Evidence {
            path: "src/lib.rs".into(),
            pointer: "/source:3".into(),
            kind: crate::SourceSignal::PinocchioMutableEntrypoint
                .evidence_kind()
                .into(),
            digest: "test".into(),
        },
    ];
    let result = super::pinocchio::framework_syntax_result(&value, &Dataset::bundled().unwrap());
    assert_eq!(result.outcome, Outcome::Pass);
    assert!(result.evidence.iter().any(|ev| ev.path == "Cargo.lock"));
    assert!(result.evidence.iter().any(|ev| ev.path == "src/lib.rs"));
}
