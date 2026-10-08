//! SC400: explicitly bound IDL and resolved reader combinations; unreviewed combinations remain unknown.

use crate::evaluation::{advice, advice_at, parsed, rule_result};
use crate::{CheckResult, Dataset, IdlInput, Outcome, Project, Severity};

pub(super) fn idl_result(idl: &IdlInput, project: &Project, data: &Dataset) -> CheckResult {
    let mut r = rule_result(
        data,
        "SC400",
        &idl.id,
        "read Anchor IDL",
        idl.evidence.clone(),
    );
    let Some(client) = project.clients.iter().find(|c| c.id == idl.client) else {
        return r;
    };
    let Some(version) = parsed(client.resolved_dependencies.get(&idl.reader_package)) else {
        r.summary = format!(
            "IDL schema {}; exact resolved version for {} is unavailable.",
            idl.schema, idl.reader_package
        );
        r.remediation = Some(advice(
            "Provide package-lock evidence for the explicitly bound IDL reader.",
        ));
        return r;
    };
    let supported = match (idl.schema.as_str(), idl.reader_package.as_str()) {
        ("anchor-legacy", "@coral-xyz/anchor") => version.major == 0 && version.minor < 30,
        (s, "@coral-xyz/anchor") if s.starts_with("anchor-spec-") => {
            version.major == 0 && version.minor >= 30
        }
        (s, "@anchor-lang/core") if s.starts_with("anchor-spec-") => version.major == 1,
        _ => false,
    };
    r.summary = format!(
        "IDL schema {}; reader {} {}.",
        idl.schema, idl.reader_package, version
    );
    if idl.schema == "unknown" {
        return r;
    }
    if supported {
        r.outcome = Outcome::Pass;
        r.title = "IDL schema and reader are a reviewed combination".into();
    } else if idl.schema.starts_with("anchor-spec-")
        && idl.reader_package == "@coral-xyz/anchor"
        && version.major == 0
        && version.minor < 30
    {
        r.outcome = Outcome::Finding;
        r.severity = Some(Severity::Error);
        r.title = "IDL schema and reader are not a reviewed combination".into();
        r.remediation=Some(advice_at("Use a reader generation matching the IDL schema, or convert/regenerate the IDL with a pinned Anchor toolchain.", &idl.path));
    } else {
        r.title = "IDL and reader combination is outside the reviewed matrix".into();
    }
    r
}
