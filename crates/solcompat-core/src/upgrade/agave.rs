//! Reviewed Agave target status; operator changes do not imply program incompatibility.
use super::common::{remediation, result};
use crate::{CheckResult, Severity};
use semver::Version;
const AGAVE_CHANGELOG: &str = "https://github.com/anza-xyz/agave/blob/master/CHANGELOG.md";

pub(super) fn evaluate(agave_target_version: &Version) -> Vec<CheckResult> {
    let agave_target_version = agave_target_version.clone();
    let mut results = Vec::new();
    if agave_target_version >= Version::new(4, 4, 0) {
        results.push(result(
            "UP200",
            "Agave 4.4 preview status",
            "agave-toolchain".into(),
            "Agave 4.4 is a preview target",
            format!("Selected Agave target {agave_target_version} is on the reviewed pre-release line."),
            "Agave 4.4 includes validator/operator-facing breaking changes such as scheduler-bindings v5 and removed experimental XDP flags. These do not automatically imply an on-chain program incompatibility.",
            Severity::Warning,
            vec![],
            AGAVE_CHANGELOG,
            Some(remediation(
                "Use Agave 4.3 for a stable production target, or validate 4.4 operator integrations in a staging environment.",
                "validator, RPC, Geyser, and deployment infrastructure".into(),
                vec![],
            )),
        ));
    }

    results
}
