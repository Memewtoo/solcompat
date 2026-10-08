//! SC102: existing ELF version eligibility for the selected loader and operation policy. Unknown flags/policies remain unknown.

use crate::evaluation::{advice_at, rule_result};
use crate::{AnalysisTarget, Artifact, CheckResult, Dataset, Outcome, Severity};

pub(super) fn artifact_result(
    a: &Artifact,
    data: &Dataset,
    target: &AnalysisTarget,
) -> CheckResult {
    let mut r = rule_result(data, "SC102", &a.id, &a.operation, a.evidence.clone());
    let Some(v) = &a.sbpf_version else {
        r.summary = format!(
            "ELF e_flags={} is not a recognized sBPF version.",
            a.elf_flags
        );
        return r;
    };
    if target.id == "future-sbpfv3-deployment" && a.operation == "execute" {
        r.conditional = true;
        r.outcome = Outcome::NotApplicable;
        r.title = "Scenario does not restrict existing-program execution".into();
        r.summary = format!(
            "Conditional scenario applies to deploy/upgrade/finalize; artifact is sBPF{v}."
        );
        return r;
    }
    let Some(policy) = target
        .artifact_policies
        .iter()
        .find(|policy| policy.loader == a.loader && policy.operations.contains(&a.operation))
    else {
        r.summary = format!(
            "Artifact is sBPF{v}; target {} has no policy for {} with {}.",
            target.id, a.operation, a.loader
        );
        r.remediation = Some(advice_at(
            "Select a target or target file with a reviewed policy for this loader and operation.",
            &a.path,
        ));
        return r;
    };
    r.conditional = target.conditional;
    if policy.allowed_sbpf_versions.contains(v) {
        r.outcome = Outcome::Pass;
        r.title = "Artifact meets the selected deployment policy".into();
        r.summary = format!(
            "Artifact is sBPF{v}; the selected target allows it for {} with {}.",
            a.operation, a.loader
        );
    } else {
        r.outcome = Outcome::Finding;
        r.severity = Some(Severity::Error);
        r.title = "Artifact is ineligible under the selected deployment policy".into();
        r.summary = format!(
            "Target {} allows [{}] for {} with {}; artifact is sBPF{v}.",
            target.id,
            policy.allowed_sbpf_versions.join(", "),
            a.operation,
            a.loader
        );
        r.remediation=Some(advice_at("Rebuild explicitly for sBPFv3 and bind the newly produced artifact before reassessing deployment eligibility.", &a.path));
    }
    r
}
