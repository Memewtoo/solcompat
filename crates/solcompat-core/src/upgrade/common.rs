//! Upgrade result and migration-action formatting.
use crate::{CheckResult, Outcome, Remediation, Severity};

// Keeping report fields explicit makes each release-boundary declaration auditable.
#[allow(clippy::too_many_arguments)]
pub(super) fn result(
    id: &str,
    name: &str,
    subject: String,
    title: &str,
    summary: String,
    explanation: &str,
    severity: Severity,
    evidence: Vec<crate::Evidence>,
    source: &str,
    remediation: Option<Remediation>,
) -> CheckResult {
    CheckResult {
        rule_id: id.into(),
        rule_name: name.into(),
        rule_revision: crate::catalog::rule(id)
            .expect("catalog rule")
            .binary_revision()
            .expect("binary metadata"),
        subject,
        operation: "upgrade".into(),
        conditional: false,
        outcome: Outcome::Finding,
        severity: Some(severity),
        title: title.into(),
        summary,
        explanation: explanation.into(),
        evidence,
        sources: vec![source.into()],
        remediation,
        suppression: None,
    }
}

pub(super) fn remediation(summary: &str, location: String, steps: Vec<String>) -> Remediation {
    Remediation {
        summary: summary.into(),
        location,
        steps,
        example: None,
        verification: vec![
            "Build the program, review regenerated IDLs and client types, then run the project tests.".into(),
            "Rerun `solcompat` and `solcompat upgrade` after updating lockfiles.".into(),
        ],
    }
}
