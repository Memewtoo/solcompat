//! Report finalization, suppression policy, and semantic verdict selection.

use crate::{CheckResult, Counts, Outcome, Report, Severity, Suppression};
use anyhow::{ensure, Result};

pub(crate) fn apply_suppressions(
    results: &mut [CheckResult],
    suppressions: &[Suppression],
) -> Result<()> {
    for suppression in suppressions {
        let matches: Vec<_> = results
            .iter()
            .enumerate()
            .filter(|(_, result)| {
                result.rule_id == suppression.rule && result.subject == suppression.subject
            })
            .map(|(i, _)| i)
            .collect();
        ensure!(
            !matches.is_empty(),
            "suppression does not match an evaluated rule/subject: {} {}",
            suppression.rule,
            suppression.subject
        );
        ensure!(matches.len()==1,"suppression is ambiguous for rule/subject: {} {}; use a unique read/component identity",suppression.rule,suppression.subject);
        let result = &mut results[matches[0]];
        ensure!(
            result.suppression.is_none(),
            "duplicate suppression for {}",
            suppression.subject
        );
        ensure!(
            matches!(result.outcome, Outcome::Finding | Outcome::Unknown),
            "suppression must target a finding or unknown: {}",
            suppression.subject
        );
        result.suppression = Some(suppression.clone());
    }
    Ok(())
}

impl Report {
    /// Sort findings, recalculate counts, and apply the configured exit policy.
    /// Suppressed results are counted separately and do not affect the exit code.
    pub fn finish(&mut self) {
        self.results
            .sort_by(|a, b| (&a.subject, &a.rule_id).cmp(&(&b.subject, &b.rule_id)));
        self.counts = Counts::default();
        self.exit_code = 0;
        for result in &self.results {
            if result.suppression.is_some() {
                self.counts.suppressed += 1;
                continue;
            }
            match result.outcome {
                Outcome::Pass => self.counts.passed += 1,
                Outcome::Unknown => {
                    self.counts.unknown += 1;
                    if self.policy.deny_unknown {
                        self.exit_code = 1;
                    }
                }
                Outcome::NotApplicable => self.counts.not_applicable += 1,
                Outcome::Skipped => {
                    self.counts.skipped += 1;
                    if self.policy.deny_unknown {
                        self.exit_code = 1;
                    }
                }
                Outcome::Finding => match result.severity {
                    Some(Severity::Error) => {
                        self.counts.errors += 1;
                        self.exit_code = 1;
                    }
                    Some(Severity::Warning) => {
                        self.counts.warnings += 1;
                        if self.policy.deny_warnings {
                            self.exit_code = 1;
                        }
                    }
                    _ => self.counts.info += 1,
                },
            }
        }
    }
}

/// Semantic verdict used by every terminal renderer. This is not a new wire field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Inventory,
    BreakingChanges,
    UpgradeReview,
    Incompatible,
    InputPolicyFailure,
    Incomplete,
    WarningPolicyFailure,
    Warnings,
    Notes,
    Suppressions,
    NoUpgradeAdvice,
    NoChecks,
    Compatible,
}

impl Report {
    /// Select the existing verdict using finalized counts and report policy.
    /// Preserve priority between upgrade advice, errors, unknowns, and warnings.
    pub fn verdict(&self) -> Verdict {
        if self.command == "inspect" {
            return Verdict::Inventory;
        }
        let visible = self
            .results
            .iter()
            .any(|result| result.outcome != Outcome::NotApplicable);
        if self.command == "upgrade" && self.counts.errors > 0 {
            Verdict::BreakingChanges
        } else if self.command == "upgrade" && self.counts.warnings > 0 {
            Verdict::UpgradeReview
        } else if self.counts.errors > 0 {
            Verdict::Incompatible
        } else if (self.counts.unknown > 0 || self.counts.skipped > 0) && self.policy.deny_unknown {
            Verdict::InputPolicyFailure
        } else if self.counts.unknown > 0 || self.counts.skipped > 0 {
            Verdict::Incomplete
        } else if self.counts.warnings > 0 && self.policy.deny_warnings {
            Verdict::WarningPolicyFailure
        } else if self.counts.warnings > 0 {
            Verdict::Warnings
        } else if self.counts.info > 0 {
            Verdict::Notes
        } else if self.counts.suppressed > 0 {
            Verdict::Suppressions
        } else if !visible && self.command == "upgrade" {
            Verdict::NoUpgradeAdvice
        } else if !visible {
            Verdict::NoChecks
        } else {
            Verdict::Compatible
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatasetIdentity, Policy, Project, Verdict};

    fn finding(outcome: Outcome, severity: Option<Severity>) -> CheckResult {
        CheckResult {
            rule_id: "SC201".into(),
            rule_name: "test".into(),
            rule_revision: 1,
            subject: "client/read".into(),
            operation: "getBlock".into(),
            conditional: false,
            outcome,
            severity,
            title: "test".into(),
            summary: String::new(),
            explanation: String::new(),
            evidence: vec![],
            sources: vec![],
            remediation: None,
            suppression: None,
        }
    }
    fn report(results: Vec<CheckResult>, policy: crate::Policy) -> Report {
        Report {
            schema_version: 1,
            command: "check".into(),
            analysis_mode: "static".into(),
            target: "local".into(),
            target_digest: None,
            dataset: DatasetIdentity {
                revision: "test".into(),
                digest: "test".into(),
            },
            project: Project {
                name: "test".into(),
                clients: vec![],
                programs: vec![],
                idls: vec![],
                artifacts: vec![],
                tools: vec![],
            },
            results,
            counts: Counts::default(),
            policy,
            exit_code: 0,
        }
    }

    #[test]
    fn outcome_policy_table_preserves_verdicts_counts_and_exits() {
        for (outcome, severity, default, strict, default_exit, strict_exit) in [
            (
                Outcome::Pass,
                None,
                Verdict::Compatible,
                Verdict::Compatible,
                0,
                0,
            ),
            (
                Outcome::Finding,
                Some(Severity::Error),
                Verdict::Incompatible,
                Verdict::Incompatible,
                1,
                1,
            ),
            (
                Outcome::Finding,
                Some(Severity::Warning),
                Verdict::Warnings,
                Verdict::WarningPolicyFailure,
                0,
                1,
            ),
            (
                Outcome::Finding,
                Some(Severity::Info),
                Verdict::Notes,
                Verdict::Notes,
                0,
                0,
            ),
            (Outcome::Finding, None, Verdict::Notes, Verdict::Notes, 0, 0),
            (
                Outcome::Unknown,
                None,
                Verdict::Incomplete,
                Verdict::InputPolicyFailure,
                0,
                1,
            ),
            (
                Outcome::NotApplicable,
                None,
                Verdict::NoChecks,
                Verdict::NoChecks,
                0,
                0,
            ),
            // Skipped checks fail a strict completeness policy just like unknowns.
            (
                Outcome::Skipped,
                None,
                Verdict::Incomplete,
                Verdict::InputPolicyFailure,
                0,
                1,
            ),
        ] {
            for (policy, expected, exit) in [
                (Policy::default(), default, default_exit),
                (
                    Policy {
                        deny_warnings: true,
                        deny_unknown: true,
                    },
                    strict,
                    strict_exit,
                ),
            ] {
                let mut value = report(vec![finding(outcome, severity)], policy);
                value.finish();
                assert_eq!(value.verdict(), expected);
                assert_eq!(value.exit_code, exit);
                let c = &value.counts;
                assert_eq!(
                    c.passed
                        + c.errors
                        + c.warnings
                        + c.info
                        + c.unknown
                        + c.not_applicable
                        + c.skipped
                        + c.suppressed,
                    1
                );
                let counts = value.counts.clone();
                value.finish();
                assert_eq!(value.counts, counts);
            }
        }
    }

    #[test]
    fn suppression_rejects_ambiguous_matches_and_invalid_targets() {
        let mut results = vec![
            finding(Outcome::Finding, Some(Severity::Error)),
            finding(Outcome::Finding, Some(Severity::Warning)),
        ];
        let suppression = Suppression {
            rule: "SC201".into(),
            subject: "client/read".into(),
            reason: "accepted".into(),
        };
        assert!(
            apply_suppressions(&mut results, std::slice::from_ref(&suppression))
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
        assert!(results.iter().all(|result| result.suppression.is_none()));
        results[1].subject = "other/read".into();
        apply_suppressions(&mut results, std::slice::from_ref(&suppression)).unwrap();
        assert!(results[0].suppression.is_some());
        assert!(results[1].suppression.is_none());
        assert!(apply_suppressions(&mut results, &[suppression])
            .unwrap_err()
            .to_string()
            .contains("duplicate suppression"));
        let mut value = report(results, Policy::default());
        value.finish();
        assert_eq!(value.counts.suppressed, 1);
        assert_eq!(value.counts.warnings, 1);
        assert_eq!(value.counts.errors, 0);
        assert_eq!(value.verdict(), Verdict::Warnings);
        value.results.remove(1);
        value.finish();
        assert_eq!(value.verdict(), Verdict::Suppressions);
        let mut passing = vec![finding(Outcome::Pass, None)];
        let suppression = Suppression {
            rule: "SC201".into(),
            subject: "client/read".into(),
            reason: "accepted".into(),
        };
        assert!(apply_suppressions(&mut passing, &[suppression]).is_err());
    }

    #[test]
    fn upgrade_and_inventory_keep_their_existing_verdict_priority() {
        let mut value = report(
            vec![
                finding(Outcome::Unknown, None),
                finding(Outcome::Finding, Some(Severity::Warning)),
            ],
            Policy {
                deny_warnings: true,
                deny_unknown: true,
            },
        );
        value.finish();
        assert_eq!(value.verdict(), Verdict::InputPolicyFailure);
        value.command = "upgrade".into();
        assert_eq!(value.verdict(), Verdict::UpgradeReview);
        value
            .results
            .push(finding(Outcome::Finding, Some(Severity::Error)));
        value.finish();
        assert_eq!(value.verdict(), Verdict::BreakingChanges);
        value.command = "inspect".into();
        assert_eq!(value.verdict(), Verdict::Inventory);
        let mut empty = report(
            vec![finding(Outcome::NotApplicable, None)],
            Policy::default(),
        );
        empty.command = "upgrade".into();
        empty.finish();
        assert_eq!(empty.verdict(), Verdict::NoUpgradeAdvice);
    }
}
