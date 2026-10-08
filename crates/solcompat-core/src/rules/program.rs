//! SC001 and SC103: selected SBF compiler and build prerequisites. Host tools are not substituted for missing selections.

use crate::evaluation::{
    advice, advice_at, declares, parse_version, parsed, program_base_evidence, resolved,
    rule_result,
};
use crate::{CheckResult, Dataset, Outcome, Program, Remediation, Severity};
use semver::Version;

pub(super) fn rust_requirement(program: &Program, data: &Dataset) -> CheckResult {
    let mut rust = rule_result(
        data,
        "SC001",
        &program.id,
        "compile program",
        program_base_evidence(program),
    );
    match (
        program.rust_version.as_deref().and_then(parse_version),
        parsed(program.sbf_rust_version.as_ref()),
    ) {
        (Some(requirement), Some(compiler)) if compiler >= requirement => {
            rust.outcome = Outcome::Pass;
            rust.title = "Selected SBF compiler satisfies the package Rust requirement".into();
            rust.summary =
                format!("Package rust-version {requirement}; selected SBF compiler {compiler}.");
            rust.remediation = None;
        }
        (Some(requirement), Some(compiler)) => {
            rust.outcome = Outcome::Finding;
            rust.severity = Some(Severity::Error);
            rust.title = "Selected SBF compiler does not satisfy rust-version".into();
            rust.summary =
                format!("Package rust-version {requirement}; selected SBF compiler {compiler}.");
            rust.remediation=Some(Remediation{summary:"Select an SBF toolchain whose compiler satisfies the package rust-version, or lower the requirement only after validating the source.".into(),location:program.manifest.clone(),steps:vec![],example:None,verification:vec!["Build the program with the selected SBF toolchain in the explicit build workflow.".into()]});
        }
        (None, _) if program.rust_version.is_none() => {
            rust.outcome = Outcome::NotApplicable;
            rust.title = "No package rust-version requirement declared".into();
            rust.summary = "Cargo does not declare a minimum Rust version for this program.".into();
        }
        _ => {
            rust.summary="A declared rust-version and an explicit SBF compiler version are both required; host rustc is not substituted.".into();
            rust.remediation=Some(advice("Set sbf_rust_version for this program from the selected platform-tools/compiler identity."));
        }
    }
    rust
}

pub(super) fn sbpf_prerequisites(program: &Program, data: &Dataset) -> CheckResult {
    let mut arch = rule_result(
        data,
        "SC103",
        &program.id,
        "build sBPFv3",
        program_base_evidence(program),
    );
    if program.sbpf_arch.as_deref() != Some("v3") {
        arch.outcome = Outcome::NotApplicable;
        arch.title = "sBPFv3 was not selected".into();
        arch.summary =
            "Set sbpf_arch = \"v3\" only when evaluating an sBPFv3 build profile.".into();
    } else {
        let builder = parsed(program.cargo_build_sbf_version.as_ref());
        let platform = parsed(program.platform_tools_version.as_ref());
        let pin = resolved(program, "pinocchio");
        let syscall = resolved(program, "solana-define-syscall");
        let has_syscall = declares(program, "solana-define-syscall");
        let ok = builder
            .as_ref()
            .is_some_and(|v| *v >= Version::new(4, 2, 0))
            && platform
                .as_ref()
                .is_some_and(|v| *v >= Version::new(1, 53, 0))
            && (program.framework != "pinocchio"
                || pin.as_ref().is_some_and(|v| *v >= Version::new(0, 10, 0)))
            && (!has_syscall
                || syscall
                    .as_ref()
                    .is_some_and(|v| *v >= Version::new(2, 3, 0)));
        if builder.is_none()
            || platform.is_none()
            || (program.framework == "pinocchio" && pin.is_none())
            || (has_syscall && syscall.is_none())
        {
            arch.summary="sBPFv3 requires exact builder/platform versions and resolved framework prerequisites.".into();
            arch.remediation=Some(advice_at("Declare cargo_build_sbf_version and platform_tools_version; provide lock or metadata evidence for relevant crates.", &program.manifest));
        } else if ok {
            arch.outcome = Outcome::Pass;
            arch.title = "Reviewed sBPFv3 build prerequisites are met".into();
            arch.summary = format!(
                "cargo-build-sbf {}; platform-tools {}.",
                builder
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                platform
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            );
        } else {
            arch.outcome = Outcome::Finding;
            arch.severity = Some(Severity::Error);
            arch.title = "sBPFv3 build prerequisites are not met".into();
            arch.summary =
                "One or more selected versions are below the reviewed sBPFv3 minimums.".into();
            arch.remediation=Some(advice_at("Use cargo-build-sbf 4.2+, platform-tools 1.53+, Pinocchio 0.10+ when applicable, and solana-define-syscall 2.3+ when present.", &program.manifest));
        }
    }
    arch
}
