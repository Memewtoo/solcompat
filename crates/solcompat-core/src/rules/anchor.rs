//! SC003 and SC302: exact Anchor release-line relationships. Mismatches remain advisory; missing versions remain unknown.

use crate::evaluation::{
    advice_at, declared_requirement, parsed, program_base_evidence, resolved, rule_result,
};
use crate::{CheckResult, Dataset, Outcome, Program, Severity};

pub(super) fn cli_alignment(program: &Program, data: &Dataset) -> CheckResult {
    let mut cli = rule_result(
        data,
        "SC003",
        &program.id,
        "Anchor build/IDL generation",
        program_base_evidence(program),
    );
    if program.framework != "anchor" {
        cli.outcome = Outcome::NotApplicable;
        cli.title = "Program is not identified as Anchor".into();
        cli.summary = format!("Detected framework: {}.", program.framework);
    } else {
        match (
            parsed(program.anchor_cli_version.as_ref()),
            resolved(program, "anchor-lang"),
        ) {
            (Some(a), Some(b)) if (a.major, a.minor) == (b.major, b.minor) => {
                cli.outcome = Outcome::Pass;
                cli.title = "Anchor CLI and anchor-lang release lines align".into();
                cli.summary = format!("Anchor CLI {a}; anchor-lang {b}.");
            }
            (Some(a), Some(b)) => {
                cli.outcome = Outcome::Finding;
                cli.severity = Some(Severity::Warning);
                cli.title = "Anchor CLI differs from Cargo-resolved anchor-lang".into();
                cli.summary = match declared_requirement(program, "anchor-lang") {
                    Some(requirement) => format!(
                        "Cargo accepted anchor-lang {b} for requirement {requirement}; selected Anchor CLI is {a}."
                    ),
                    None => format!("Anchor CLI {a}; resolved anchor-lang {b}."),
                };
                cli.explanation = "The resolved crate satisfies Cargo's declared version range. This establishes dependency-range compatibility, but it does not prove that the selected CLI and resolved framework release are fully aligned or that current source built successfully. Anchor release guidance updates the CLI and framework crates together, so the drift remains advisory.".into();
                let mut remediation = advice_at(
                    "Consider aligning the selected CLI and resolved Anchor packages; keep the current combination if its successful build and generated outputs are intentionally accepted.",
                    &program.manifest,
                );
                remediation.steps = vec![
                    format!("To keep resolved anchor-lang {b}, select Anchor CLI {b} and align the other Anchor packages to that release."),
                    format!("To keep Anchor CLI {a}, intentionally constrain anchor-lang to the intended {a} release, then update Cargo.lock. Use an exact requirement only when strict pinning is intended."),
                    "Run the normal Anchor build, review the regenerated IDL/client output, and rerun SolCompat.".into(),
                ];
                remediation.verification = vec![
                    "Confirm SC003 reports aligned release lines and the project build/tests pass with the selected versions.".into(),
                ];
                cli.remediation = Some(remediation);
            }
            (Some(anchor_cli), None) => {
                cli.title = "Resolved anchor-lang version is unavailable".into();
                cli.summary = match declared_requirement(program, "anchor-lang") {
                    Some(requirement) => format!(
                        "Anchor CLI {anchor_cli} is selected; anchor-lang requirement {requirement} is declared but not resolved."
                    ),
                    None => format!(
                        "Anchor CLI {anchor_cli} is selected; no resolved anchor-lang version is available."
                    ),
                };
                let mut remediation = advice_at(
                    "Resolve the program dependencies, then rerun SolCompat with Cargo.lock or captured cargo metadata.",
                    &program.manifest,
                );
                remediation.steps = vec![
                    "If Cargo.lock is absent, run anchor build for the normal project build, or run cargo generate-lockfile when only dependency resolution is needed.".into(),
                    "Rerun solcompat check --detailed; SolCompat will use an unambiguous crates.io version from Cargo.lock.".into(),
                    "If Cargo.lock contains multiple anchor-lang versions, capture cargo metadata --format-version 1 and pass the JSON file with --cargo-metadata.".into(),
                ];
                remediation.example = Some(
                    "# Full Anchor build:\nanchor build\n\n# Or dependency resolution only:\ncargo generate-lockfile\n\nsolcompat check --detailed"
                        .into(),
                );
                remediation.verification = vec![
                    "Confirm SC003 reports the selected Anchor CLI and the resolved anchor-lang version.".into(),
                ];
                cli.remediation = Some(remediation);
            }
            (None, Some(anchor_lang)) => {
                cli.title = "Anchor CLI version is unavailable".into();
                cli.summary = format!(
                    "anchor-lang {anchor_lang} is resolved; no Anchor CLI version is selected or observed."
                );
                cli.remediation = Some(advice_at(
                    "Set [toolchain].anchor_version in Anchor.toml, configure anchor_cli_version for this program, or use --probe-tools with an installed native Anchor executable.",
                    &program.manifest,
                ));
            }
            (None, None) => {
                cli.title = "Anchor CLI and resolved anchor-lang versions are unavailable".into();
                cli.summary = "SC003 requires a selected or observed Anchor CLI version and a resolved anchor-lang version.".into();
                cli.remediation = Some(advice_at(
                    "Supply the Anchor CLI version and Cargo.lock or captured cargo metadata resolving anchor-lang.",
                    &program.manifest,
                ));
            }
        }
    }
    cli
}

pub(super) fn crate_alignment(program: &Program, data: &Dataset) -> CheckResult {
    let mut crates = rule_result(
        data,
        "SC302",
        &program.id,
        "compile Anchor program",
        program_base_evidence(program),
    );
    if program.framework != "anchor" {
        crates.outcome = Outcome::NotApplicable;
        crates.title = "Program is not identified as Anchor".into();
        crates.summary = format!("Detected framework: {}.", program.framework);
    } else {
        match (
            resolved(program, "anchor-lang"),
            resolved(program, "anchor-spl"),
        ) {
            (Some(a), Some(b)) if (a.major, a.minor) == (b.major, b.minor) => {
                crates.outcome = Outcome::Pass;
                crates.title = "Anchor crate release lines align".into();
                crates.summary = format!("anchor-lang {a}; anchor-spl {b}.");
            }
            (Some(a), Some(b)) => {
                crates.outcome = Outcome::Finding;
                crates.severity = Some(Severity::Warning);
                crates.title = "Anchor crate release lines differ".into();
                crates.summary = format!("anchor-lang {a}; anchor-spl {b}.");
                crates.remediation=Some(advice_at("Align anchor-lang and anchor-spl to the same reviewed release line unless the chosen combination is explicitly supported.", &program.manifest));
            }
            (_, None) => {
                crates.outcome = Outcome::NotApplicable;
                crates.title = "anchor-spl is not a resolved dependency".into();
                crates.summary = "No anchor-spl relationship is present to compare.".into();
            }
            _ => {
                crates.summary =
                    "Resolved Anchor crate versions are required for comparison.".into();
            }
        }
    }
    crates
}
