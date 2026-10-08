//! SC104: reviewed Pinocchio API boundaries using resolved versions and typed source signals; absent signals do not prove migration completeness.

use crate::evaluation::{advice_at, program_base_evidence, resolved, rule_result};
use crate::{CheckResult, Dataset, Outcome, Program, Remediation, Severity, SourceSignal};
use semver::Version;

pub(super) fn framework_syntax_result(program: &Program, data: &Dataset) -> CheckResult {
    let mut result = syntax_result(program, data);
    if program
        .evidence
        .iter()
        .any(|ev| ev.pointer == "/features/default-static-profile")
    {
        result.summary.push_str(" Source profile: package default features, non-test; no-entrypoint guards are evaluated in that profile. Custom build flags and dependency feature unification are not inferred.");
    }
    result
}

fn syntax_result(program: &Program, data: &Dataset) -> CheckResult {
    let mut evidence = program
        .evidence
        .iter()
        .filter(|evidence| SourceSignal::is_pinocchio_evidence(&evidence.kind))
        .cloned()
        .collect::<Vec<_>>();
    evidence.sort_by(|a, b| (&a.path, &a.pointer).cmp(&(&b.path, &b.pointer)));
    evidence.dedup_by(|a, b| a.path == b.path && a.pointer == b.pointer);
    evidence.extend(program_base_evidence(program));
    let mut result = rule_result(
        data,
        "SC104",
        &program.id,
        "compile Pinocchio program",
        evidence,
    );
    if program.framework != "pinocchio" {
        result.outcome = Outcome::NotApplicable;
        result.title = "Program is not identified as Pinocchio".into();
        result.summary = format!("Detected framework: {}.", program.framework);
        return result;
    }
    let Some(current) = resolved(program, "pinocchio") else {
        result.title = "Resolved Pinocchio version is unavailable".into();
        result.summary = "Source syntax was inspected, but Cargo.lock does not prove the Pinocchio release line.".into();
        result.remediation = Some(advice_at(
            "Generate Cargo.lock or provide cargo metadata, then rerun SolCompat.",
            &program.manifest,
        ));
        return result;
    };

    if program
        .evidence
        .iter()
        .any(|ev| ev.kind == "unresolved-rust")
    {
        result.title = "Pinocchio source selection is unresolved".into();
        result.summary = "A module could not be parsed or resolved, or conditional code affects a reviewed framework pattern. Static scanning cannot establish the selected source API.".into();
        result.remediation = Some(advice_at("Review the affected source, Cargo features, and module paths, then validate with solcompat --build. Build success is reported separately by SC100; it does not resolve SC104’s static source-selection limitation.", &program.manifest));
        return result;
    }

    let legacy_types = SourceSignal::PinocchioAccountInfo.present(program)
        || SourceSignal::PinocchioPubkey.present(program);
    let immutable_entrypoint = SourceSignal::PinocchioImmutableEntrypoint.present(program);
    let mutable_entrypoint = SourceSignal::PinocchioMutableEntrypoint.present(program);
    let source_location = result
        .evidence
        .iter()
        .find(|ev| SourceSignal::is_pinocchio_evidence(&ev.kind))
        .map_or_else(
            || program.manifest.clone(),
            |evidence| evidence.path.clone(),
        );
    let v010 = Version::new(0, 10, 0);
    let v011 = Version::new(0, 11, 0);

    if current < v010 {
        if immutable_entrypoint || mutable_entrypoint {
            result.outcome = Outcome::Finding;
            result.severity = Some(Severity::Error);
            result.title = "Pinocchio source expects the 0.10+ AccountView API".into();
            result.summary = format!(
                "Resolved Pinocchio {current}; source uses Address/AccountView entrypoint syntax introduced on the 0.10 release line."
            );
            result.remediation = Some(advice_at(
                "Upgrade Pinocchio and its companion crates to a compatible 0.10+ release, update Cargo.lock, and rebuild.",
                source_location,
            ));
        } else {
            result.outcome = Outcome::Finding;
            result.severity = Some(Severity::Warning);
            result.title = "Pinocchio 0.9 crosses two source API migrations".into();
            result.summary = if legacy_types {
                format!(
                    "Resolved Pinocchio {current}; AccountInfo/Pubkey syntax matches the 0.9 API. Migrating to 0.10 changes these to AccountView/Address, and 0.11 then requires a mutable AccountView slice."
                )
            } else {
                format!(
                    "Resolved Pinocchio {current}; upgrading crosses the 0.10 AccountInfo/Pubkey replacement and the 0.11 mutable AccountView boundary."
                )
            };
            result.remediation = Some(Remediation {
                summary: "Apply the Pinocchio 0.10 type migration before the separate 0.11 mutability migration.".into(),
                location: source_location,
                steps: vec![
                    "For 0.10, replace Pubkey with Address and AccountInfo with AccountView; update key() calls to address() and revise affected account APIs.".into(),
                    "Validate the 0.10 form with accounts: &[AccountView] before crossing the next boundary.".into(),
                    "For 0.11, change the entrypoint to accounts: &mut [AccountView], update mutating account references, and review resize traits/features.".into(),
                ],
                example: Some("0.9: (&Pubkey, &[AccountInfo]) -> 0.10: (&Address, &[AccountView]) -> 0.11: (&Address, &mut [AccountView])".into()),
                verification: vec![
                    "Update Cargo.lock, run solcompat --build at the selected target release, and run tests covering account mutation and CPI paths.".into(),
                ],
            });
        }
    } else if current < v011 {
        if legacy_types {
            result.outcome = Outcome::Finding;
            result.severity = Some(Severity::Error);
            result.title = "Pinocchio 0.10 source still uses the removed 0.9 account types".into();
            result.summary = format!(
                "Resolved Pinocchio {current}; qualified AccountInfo or Pubkey syntax from the 0.9 API was observed."
            );
            result.remediation = Some(Remediation {
                summary: "Migrate the current source to the Pinocchio 0.10 Address and AccountView API.".into(),
                location: source_location,
                steps: vec![
                    "Replace Pubkey with Address and AccountInfo with AccountView.".into(),
                    "Update key() calls to address() and revise borrow, data, owner, and CPI account access as required by the 0.10 API.".into(),
                    "Keep the entrypoint account slice immutable on 0.10; make it mutable only when upgrading to 0.11+.".into(),
                ],
                example: Some("fn process_instruction(program_id: &Address, accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult".into()),
                verification: vec!["Run solcompat --build with the resolved 0.10 toolchain and the program tests.".into()],
            });
        } else if mutable_entrypoint {
            result.outcome = Outcome::Finding;
            result.severity = Some(Severity::Error);
            result.title = "Pinocchio source expects the 0.11 mutable entrypoint API".into();
            result.summary = format!(
                "Resolved Pinocchio {current}; source uses &mut [AccountView], which belongs to the 0.11+ entrypoint contract."
            );
            result.remediation = Some(advice_at(
                "Upgrade Pinocchio and companion crates to compatible 0.11-era releases, update Cargo.lock, and rebuild.",
                source_location,
            ));
        } else {
            result.outcome = Outcome::Finding;
            result.severity = Some(Severity::Warning);
            result.title = "Pinocchio 0.11 mutable-account migration is required".into();
            result.summary = if immutable_entrypoint {
                format!(
                    "Resolved Pinocchio {current}; the 0.10 Address/AccountView API is present, and 0.11 changes accounts from &[AccountView] to &mut [AccountView]."
                )
            } else {
                format!(
                    "Resolved Pinocchio {current}; source scanning did not prove a complete 0.11 entrypoint migration."
                )
            };
            result.remediation = Some(Remediation {
                summary: "Apply the Pinocchio 0.11 mutable-account and resize migration before upgrading.".into(),
                location: source_location,
                steps: vec![
                    "Change process_instruction to accept &mut [AccountView].".into(),
                    "Update assign, close, try_borrow_mut, and mutating CPI paths to receive mutable account views.".into(),
                    "If the program resizes accounts, select account-resize or unsafe-account-resize deliberately and import the corresponding trait.".into(),
                ],
                example: Some("accounts: &mut [AccountView]".into()),
                verification: vec!["Update Cargo.lock, then run solcompat --build and mutation-focused tests.".into()],
            });
        }
    } else if legacy_types {
        result.outcome = Outcome::Finding;
        result.severity = Some(Severity::Error);
        result.title = "Pinocchio source uses legacy AccountInfo or Pubkey syntax".into();
        result.summary = format!(
            "Resolved Pinocchio {current} uses Address and AccountView, but qualified 0.9-era AccountInfo or Pubkey syntax was observed."
        );
        result.remediation = Some(Remediation {
            summary: "Migrate Pinocchio types to Address and AccountView and update affected account APIs.".into(),
            location: source_location,
            steps: vec![
                "Replace Pinocchio Pubkey usage with Address and key() access with address() where applicable.".into(),
                "Replace Pinocchio AccountInfo parameters with AccountView and update borrow/data access calls.".into(),
                "Use a mutable AccountView slice for the program entrypoint on Pinocchio 0.11+.".into(),
            ],
            example: Some("fn process_instruction(program_id: &Address, accounts: &mut [AccountView], instruction_data: &[u8]) -> ProgramResult".into()),
            verification: vec!["Run solcompat --build and the program test suite.".into()],
        });
    } else if immutable_entrypoint {
        result.outcome = Outcome::Finding;
        result.severity = Some(Severity::Error);
        result.title = "Pinocchio 0.11 entrypoint uses an immutable AccountView slice".into();
        result.summary = format!(
            "Resolved Pinocchio {current}; process_instruction still accepts &[AccountView]."
        );
        result.remediation = Some(Remediation {
            summary: "Change the entrypoint to accept &mut [AccountView] and propagate mutable references only to mutating operations.".into(),
            location: source_location,
            steps: vec![
                "Change the process_instruction accounts parameter to &mut [AccountView].".into(),
                "Update assign, close, try_borrow_mut, and mutating CPI paths to receive mutable account views.".into(),
                "If the program resizes accounts, select account-resize or unsafe-account-resize deliberately and import the corresponding trait.".into(),
            ],
            example: Some("accounts: &mut [AccountView]".into()),
            verification: vec!["Run solcompat --build and tests covering every mutating instruction.".into()],
        });
    } else if mutable_entrypoint {
        result.outcome = Outcome::Pass;
        result.title = "Pinocchio entrypoint signature matches the reviewed 0.11+ pattern".into();
        result.summary = format!(
            "Resolved Pinocchio {current}; process_instruction accepts &mut [AccountView], and no legacy Pinocchio type imports were observed in the supported scan. This is a syntax check, not a complete migration or build proof."
        );
        result.remediation = None;
    } else {
        result.title = "No reviewed Pinocchio entrypoint signature was found".into();
        result.summary = format!("Resolved Pinocchio {current}; the selected source does not establish a process_instruction signature with an AccountView account slice.");
        result.remediation = Some(advice_at("Check whether package defaults enable no-entrypoint. If so, the entrypoint is excluded from this static profile. Otherwise review custom entrypoint names, types, or generated source; validate the intended program build separately with solcompat --build.", &program.manifest));
    }
    result
}
