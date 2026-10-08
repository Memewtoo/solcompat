//! Shared result, version, declaration, and evidence helpers.

pub(crate) use crate::version::parse_version;
use crate::{CheckResult, Dataset, Evidence, Outcome, Program, Remediation};
use semver::Version;

pub(crate) fn advice(summary: impl Into<String>) -> Remediation {
    Remediation {
        summary: summary.into(),
        location: "The bound component configuration or manifest; an application source location was not inferred.".into(),
        steps: vec![],
        example: None,
        verification: vec![],
    }
}
pub(crate) fn advice_at(summary: impl Into<String>, location: impl Into<String>) -> Remediation {
    let mut remediation = advice(summary);
    remediation.location = location.into();
    remediation
}

pub(crate) fn rule_result(
    data: &Dataset,
    id: &str,
    subject: &str,
    operation: &str,
    evidence: Vec<Evidence>,
) -> CheckResult {
    let record = data.rule(id);
    CheckResult {
        rule_id: id.into(),
        rule_name: record.rule_name.clone(),
        rule_revision: record.revision,
        subject: subject.into(),
        operation: operation.into(),
        conditional: false,
        outcome: Outcome::Unknown,
        severity: None,
        title: format!("{} needs additional evidence", record.rule_name),
        summary: record.notes.clone(),
        explanation: record.notes.clone(),
        evidence,
        sources: record.sources.clone(),
        remediation: None,
        suppression: None,
    }
}

pub(crate) fn parsed(value: Option<&String>) -> Option<Version> {
    parse_version(value?)
}
pub(crate) fn resolved(program: &Program, name: &str) -> Option<Version> {
    parsed(program.resolved_dependencies.get(name))
}
pub(crate) fn declares(program: &Program, name: &str) -> bool {
    program.dependencies.keys().any(|key| {
        key.rsplit('/')
            .next()
            .is_some_and(|identity| identity == name || identity.ends_with(&format!("=>{name}")))
    })
}

pub(crate) fn declared_requirement<'a>(program: &'a Program, name: &str) -> Option<&'a str> {
    program.dependencies.iter().find_map(|(key, requirement)| {
        key.strip_prefix("dependencies/").and_then(|identity| {
            (identity == name || identity.ends_with(&format!("=>{name}")))
                .then_some(requirement.as_str())
        })
    })
}

pub(crate) fn program_base_evidence(program: &Program) -> Vec<Evidence> {
    program
        .evidence
        .iter()
        .filter(|evidence| !crate::SourceSignal::is_framework_evidence(&evidence.kind))
        .cloned()
        .collect()
}

pub(crate) fn dependency<'a>(
    dependencies: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Option<&'a str> {
    dependencies.iter().find_map(|(identity, value)| {
        identity
            .split_once('/')
            .map(|(_, package)| package)
            .filter(|package| *package == name || package.ends_with(&format!("=>{name}")))
            .map(|_| value.as_str())
    })
}
