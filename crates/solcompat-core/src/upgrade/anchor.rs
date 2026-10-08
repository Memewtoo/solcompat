//! Anchor release-boundary advice from collected facts; no filesystem or tool access.
use super::common::{remediation, result};
use crate::evaluation::{dependency, program_base_evidence};
use crate::version::version;
use crate::{CheckResult, Outcome, Project, Severity, SourceSignal};
use semver::Version;
const ANCHOR_031: &str = "https://www.anchor-lang.com/docs/updates/release-notes/0-31-0";
const ANCHOR_032: &str = "https://www.anchor-lang.com/docs/updates/release-notes/0-32-0";
const ANCHOR_100: &str = "https://www.anchor-lang.com/docs/updates/release-notes/1-0-0";
const ANCHOR_120: &str = "https://www.anchor-lang.com/docs/updates/release-notes/1-2-0";

/// Evaluate Anchor boundaries in the established report order.
pub(super) fn evaluate(project: &Project, anchor_target_version: &Version) -> Vec<CheckResult> {
    let mut results = Vec::new();
    for program in project
        .programs
        .iter()
        .filter(|program| program.framework == "anchor")
    {
        let current = program
            .resolved_dependencies
            .get("anchor-lang")
            .and_then(|value| version(value, "resolved anchor-lang").ok());
        if let Some(result) = baseline(program, current.as_ref(), anchor_target_version) {
            results.push(result);
            continue;
        }
        let current = current.expect("baseline handled unresolved versions");
        results.extend(
            [
                discriminator_migration(program, &current, anchor_target_version),
                cpi_context_migration(program, &current, anchor_target_version),
                solana_crate_migration(program, &current, anchor_target_version),
                build_deploy_changes(program, &current, anchor_target_version),
                major_migration(program, &current, anchor_target_version),
            ]
            .into_iter()
            .flatten(),
        );
    }
    results.extend(typescript_package_migration(project, anchor_target_version));
    results.extend(platform_baseline(project, anchor_target_version));
    results
}

/// Reviewed release boundary for discriminator migration.
fn discriminator_migration(
    program: &crate::Program,
    current: &Version,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let current = current.clone();
    let anchor_target_version = anchor_target_version.clone();
    if current < Version::new(0, 31, 0) && anchor_target_version >= Version::new(0, 31, 0) {
        if let Some(mut evidence) = SourceSignal::AnchorDiscriminatorMethod.evidence(program) {
            let location = evidence
                .first()
                .map_or_else(|| program.manifest.clone(), |item| item.path.clone());
            evidence.extend(program_base_evidence(program));
            return Some(result(
            "UP106",
            "Anchor discriminator constant migration",
            program.id.clone(),
            "Anchor 0.31 removes the discriminator() method",
            format!(
                "Source uses `Type::discriminator()` while upgrading anchor-lang {current} to {anchor_target_version} across Anchor 0.31."
            ),
            "Anchor 0.31 removed the deprecated `Discriminator::discriminator()` method. The discriminator is exposed through the `DISCRIMINATOR` associated constant, and custom discriminators mean callers should not assume a fixed length.",
            Severity::Error,
            evidence.clone(),
            ANCHOR_031,
            Some(remediation(
                "Replace discriminator method calls with the associated constant and use its actual length.",
                location,
                vec![
                    "Replace `Type::discriminator()` with `Type::DISCRIMINATOR`.".into(),
                    "Replace hard-coded discriminator length 8 with `Type::DISCRIMINATOR.len()` where the length is part of slicing or allocation.".into(),
                ],
            )),
        ));
        }
    }

    None
}

/// Reviewed release boundary for cpi context migration.
fn cpi_context_migration(
    program: &crate::Program,
    current: &Version,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let current = current.clone();
    let anchor_target_version = anchor_target_version.clone();
    if current.major == 0 && anchor_target_version.major >= 1 {
        if let Some(mut evidence) = SourceSignal::AnchorCpiContextAccountInfo.evidence(program) {
            let location = evidence
                .first()
                .map_or_else(|| program.manifest.clone(), |item| item.path.clone());
            evidence.extend(program_base_evidence(program));
            return Some(result(
            "UP107",
            "Anchor 1.x CPI context program address migration",
            program.id.clone(),
            "Anchor 1.x CPI contexts take the program address",
            format!(
                "Source passes an AccountInfo-style value to `CpiContext::new` while upgrading anchor-lang {current} to {anchor_target_version}."
            ),
            "Anchor 1.x changes the first `CpiContext::new` and `new_with_signer` argument from a program AccountInfo to the program Pubkey.",
            Severity::Error,
            evidence.clone(),
            ANCHOR_100,
            Some(remediation(
                "Pass the CPI program address instead of calling `to_account_info()` for the first context argument.",
                location,
                vec![
                    "Replace the first `CpiContext::new` or `new_with_signer` argument with the CPI program Pubkey or declared program ID.".into(),
                    "Keep account-info conversions only for CPI account structs that still require them, then rebuild and retest the CPI.".into(),
                ],
            )),
        ));
        }
    }

    None
}

/// Reviewed release boundary for solana crate migration.
fn solana_crate_migration(
    program: &crate::Program,
    current: &Version,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let current = current.clone();
    let anchor_target_version = anchor_target_version.clone();
    if current < Version::new(0, 31, 0)
        && anchor_target_version >= Version::new(0, 31, 0)
        && dependency(&program.dependencies, "solana-program").is_some()
    {
        return Some(result(
            "UP101",
            "Anchor 0.31 Solana crate migration",
            program.id.clone(),
            "Direct solana-program dependency needs review for Anchor 0.31+",
            format!("Upgrading anchor-lang {current} to {anchor_target_version} crosses the Anchor 0.31 Agave transition."),
            "Anchor 0.31 recommends accessing Solana program APIs through anchor-lang's re-export to avoid crate version conflicts.",
            Severity::Warning,
            program_base_evidence(program),
            ANCHOR_031,
            Some(remediation(
                "Replace direct solana-program use with `anchor_lang::solana_program` where practical and align remaining Solana crates.",
                program.manifest.clone(),
                vec!["Review imports and the direct solana-program dependency before updating Anchor.".into()],
            )),
        ));
    }

    None
}

/// Reviewed release boundary for build deploy changes.
fn build_deploy_changes(
    program: &crate::Program,
    current: &Version,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let current = current.clone();
    let anchor_target_version = anchor_target_version.clone();
    if current < Version::new(0, 32, 0) && anchor_target_version >= Version::new(0, 32, 0) {
        return Some(result(
            "UP102",
            "Anchor 0.32 build and deploy changes",
            program.id.clone(),
            "Anchor 0.32 changes IDL builds and deployment behavior",
            format!("The path from anchor-lang {current} to {anchor_target_version} crosses Anchor 0.32."),
            "Anchor 0.32 requires Rust 1.89 for IDL builds, uploads the IDL during `anchor deploy` by default, and changes the verifiable-build backend.",
            Severity::Warning,
            program_base_evidence(program),
            ANCHOR_032,
            Some(remediation(
                "Update the Anchor toolchain deliberately and review CI, IDL, deploy, and verifiable-build workflows.",
                program.manifest.clone(),
                vec![
                    "Use Rust 1.89 or newer where Anchor IDLs are built.".into(),
                    "Review whether automatic IDL upload during `anchor deploy` matches the release process.".into(),
                ],
            )),
        ));
    }

    None
}

/// Reviewed release boundary for major migration.
fn major_migration(
    program: &crate::Program,
    current: &Version,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let current = current.clone();
    let anchor_target_version = anchor_target_version.clone();
    if current.major == 0 && anchor_target_version.major >= 1 {
        return Some(result(
            "UP103",
            "Anchor 1.x migration",
            program.id.clone(),
            "Anchor 1.x is a major framework and toolchain migration",
            format!("The path from anchor-lang {current} to {anchor_target_version} crosses Anchor 1.0 and targets the Solana 3.x generation."),
            "Anchor 1.0 is the first stable major release. Treat the update as a migration: align CLI and crates, regenerate IDLs and clients, then retest program behavior.",
            Severity::Warning,
            program_base_evidence(program),
            ANCHOR_100,
            Some(remediation(
                "Upgrade Anchor CLI and framework crates together, then regenerate and review generated interfaces.",
                program.manifest.clone(),
                vec![
                    "Update anchor-lang, anchor-spl, and the Anchor CLI to the same intended release line.".into(),
                    "Regenerate the IDL and all generated client types before testing.".into(),
                ],
            )),
        ));
    }
    None
}

fn baseline(
    program: &crate::Program,
    current: Option<&Version>,
    anchor_target_version: &Version,
) -> Option<CheckResult> {
    let anchor_target_version = anchor_target_version.clone();
    let Some(current) = current.cloned() else {
        return Some(CheckResult {
            rule_id: "UP100".into(),
            rule_name: "Anchor upgrade baseline".into(),
            rule_revision: crate::catalog::rule("UP100").expect("catalog rule").binary_revision().expect("binary metadata"),
            subject: program.id.clone(),
            operation: "upgrade".into(),
            conditional: false,
            outcome: Outcome::Unknown,
            severity: None,
            title: "Anchor upgrade baseline needs a resolved version".into(),
            summary: "The project declares Anchor, but Cargo.lock or cargo metadata does not prove the current anchor-lang version.".into(),
            explanation: "Upgrade advice depends on which release boundaries the project crosses.".into(),
            evidence: program_base_evidence(program),
            sources: vec![ANCHOR_031.into()],
            remediation: Some(remediation(
                "Generate Cargo.lock with `anchor build` or `cargo generate-lockfile`, then rerun the upgrade advisor.",
                program.manifest.clone(),
                vec![],
            )),
            suppression: None,
        });
    };
    if current >= anchor_target_version {
        return Some(CheckResult {
            rule_id: "UP100".into(),
            rule_name: "Anchor upgrade baseline".into(),
            rule_revision: crate::catalog::rule("UP100")
                .expect("catalog rule")
                .binary_revision()
                .expect("binary metadata"),
            subject: program.id.clone(),
            operation: "upgrade".into(),
            conditional: false,
            outcome: Outcome::Pass,
            severity: None,
            title: "Resolved Anchor version meets the selected target".into(),
            summary: format!(
                "Resolved anchor-lang {current}; selected target {anchor_target_version}."
            ),
            explanation: "No Anchor release boundary in the selected range needs migration advice."
                .into(),
            evidence: program_base_evidence(program),
            sources: vec![ANCHOR_120.into()],
            remediation: None,
            suppression: None,
        });
    }

    None
}

pub(super) fn typescript_package_migration(
    project: &Project,
    anchor_target_version: &Version,
) -> Vec<CheckResult> {
    let anchor_target_version = anchor_target_version.clone();
    let mut results = Vec::new();
    if anchor_target_version.major >= 1 {
        for client in &project.clients {
            if let Some(requirement) = dependency(&client.dependencies, "@coral-xyz/anchor") {
                results.push(result(
                    "UP104",
                    "Anchor 1.x TypeScript package rename",
                    client.id.clone(),
                    "Anchor TypeScript client package must move to @anchor-lang/core",
                    format!("{} declares @coral-xyz/anchor {requirement}; Anchor 1.x publishes the client as @anchor-lang/core.", client.manifest),
                    "The Anchor 1.0 TypeScript package rename is a source and package-manifest migration, not an automatic compatible update.",
                    Severity::Error,
                    client.evidence.clone(),
                    ANCHOR_100,
                    Some(remediation(
                        "Replace @coral-xyz/anchor with @anchor-lang/core and update imports and generated clients.",
                        client.manifest.clone(),
                        vec![
                            "Install the target @anchor-lang/core release and remove @coral-xyz/anchor.".into(),
                            "Update imports, regenerate client types, and run client tests.".into(),
                        ],
                    )),
                ));
            }
        }
    }

    results
}

pub(super) fn platform_baseline(
    project: &Project,
    anchor_target_version: &Version,
) -> Vec<CheckResult> {
    let anchor_target_version = anchor_target_version.clone();
    let mut results = Vec::new();
    if anchor_target_version >= Version::new(1, 2, 0)
        && project.programs.iter().any(|program| {
            program.framework == "anchor"
                && program
                    .resolved_dependencies
                    .get("anchor-lang")
                    .and_then(|value| version(value, "anchor-lang").ok())
                    .is_some_and(|current| current < Version::new(1, 2, 0))
        })
    {
        results.push(result(
            "UP105",
            "Anchor 1.2 platform baseline",
            "workspace".into(),
            "Anchor 1.2 recommends Solana 4.1.2",
            "The selected Anchor 1.2 target should be validated with its recommended Solana toolchain baseline.".into(),
            "Anchor's 1.2 release guidance recommends Solana 4.1.2. Keep the selected SBF build tools and deployment environment explicit in CI.",
            Severity::Info,
            vec![],
            ANCHOR_120,
            None,
        ));
    }

    results
}
