//! SC200: V1 decoder support, independently of request acceptance; exact reviewed resolution or deterministic range evidence is required.

use crate::evaluation::{advice, parse_version, rule_result};
use crate::{CheckResult, Client, Dataset, Outcome, Remediation, RpcMaximum, Severity};
use semver::Version;

pub(super) fn declared_npm_major_cap(requirement: &str, boundary: u64) -> bool {
    let requirement = requirement.trim();
    if requirement.is_empty()
        || requirement.contains("||")
        || requirement.contains('>')
        || requirement.contains('*')
    {
        return false;
    }
    let constrained = requirement.starts_with('^')
        || requirement.starts_with('~')
        || requirement
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit());
    if !constrained {
        return false;
    }
    requirement
        .trim_start_matches(['^', '~', '=', ' ', 'v'])
        .split('.')
        .next()
        .and_then(|major| major.parse::<u64>().ok())
        .is_some_and(|major| major < boundary)
}

pub(super) fn decoder_result(client: &Client, data: &Dataset) -> CheckResult {
    let mut r = rule_result(
        data,
        "SC200",
        &client.id,
        "decode transaction responses",
        client.evidence.clone(),
    );
    if client
        .required_read_versions
        .as_ref()
        .is_some_and(|versions| {
            versions
                .iter()
                .any(|value| !matches!(value.as_str(), "legacy" | "v0" | "v1"))
        })
    {
        r.summary = "A required transaction format is outside the reviewed decoder dataset.".into();
        return r;
    }
    if client
        .required_read_versions
        .as_ref()
        .is_some_and(Vec::is_empty)
    {
        r.outcome = Outcome::NotApplicable;
        r.title = "No transaction decoding requirement is in scope".into();
        r.summary = "The client explicitly declares no required transaction formats.".into();
        return r;
    }
    let explicit_v1_requirement = client
        .required_read_versions
        .as_ref()
        .is_some_and(|v| v.iter().any(|x| x == "v1"));
    let v1_accepting_rpc_read = client.rpc_reads.iter().any(|read| {
        matches!(
            read.max_supported_transaction_version,
            Some(RpcMaximum::Version(version)) if version >= 1
        )
    });
    let needs_v1 = explicit_v1_requirement || v1_accepting_rpc_read;
    if !needs_v1 {
        r.outcome = Outcome::NotApplicable;
        r.title = "No Transaction V1 decoding requirement is in scope".into();
        r.summary = "SC200 runs only when the client contract requires V1 reads; no V1 requirement was declared.".into();
        return r;
    }
    let Some(package) = &client.decoder_package else {
        r.summary = "V1 decoding is in scope, but no decoder_package is bound.".into();
        r.remediation = Some(advice(
            "Bind the package that decodes RPC transaction bodies for this client.",
        ));
        return r;
    };
    let Some(version) = client
        .resolved_dependencies
        .get(package)
        .and_then(|value| parse_version(value))
    else {
        let declared = client
            .dependencies
            .iter()
            .find_map(|(identity, requirement)| {
                identity
                    .split_once('/')
                    .filter(|(_, name)| *name == package)
                    .map(|_| requirement.as_str())
            });
        if package == "@solana/kit"
            && declared.is_some_and(|requirement| declared_npm_major_cap(requirement, 8))
        {
            let requirement = declared.expect("checked");
            r.outcome = Outcome::Finding;
            r.severity = Some(Severity::Error);
            r.title = "Declared decoder range excludes reviewed Transaction V1 support".into();
            r.explanation = "The declared major-version constraint cannot select a decoder release in the reviewed V1-capable line.".into();
            r.summary = format!(
                "{package} requirement {requirement} cannot resolve to the reviewed V1-capable 8.x line."
            );
            r.remediation = Some(Remediation {
                summary: "Upgrade @solana/kit to a reviewed 8.x release and regenerate the lockfile.".into(),
                location: client.manifest.clone(),
                steps: vec![
                    "Change the direct @solana/kit dependency to the intended 8.x release range.".into(),
                    "Regenerate the package-manager lockfile and rerun SolCompat so the installed version can also be verified.".into(),
                ],
                example: Some("npm install @solana/kit@^8".into()),
                verification: vec![
                    "Run client tests using known legacy, V0, and V1 responses.".into(),
                ],
            });
            return r;
        }
        r.summary = match declared {
            Some(requirement) => format!(
                "{package} requirement {requirement} does not prove the exact installed decoder version."
            ),
            None => format!("Exact resolved version for {package} is unavailable."),
        };
        r.remediation = Some(advice(
            "Generate the package-manager lockfile so SolCompat can verify the exact installed decoder version.",
        ));
        return r;
    };
    let supported = match package.as_str() {
        "@solana/web3.js" if version.major == 1 && version.minor < 99 => Some(false),
        "@solana/web3.js" if version.major == 1 && version.minor == 99 => Some(true),
        "@solana/web3.js" if version.major == 3 => {
            Some(version >= Version::parse("3.0.0-rc.3").expect("valid boundary"))
        }
        "@solana/kit" if version.major < 8 => Some(false),
        "@solana/kit" if version.major == 8 => Some(true),
        _ => None,
    };
    let reason = if explicit_v1_requirement {
        "the explicit transaction-version contract"
    } else {
        "an RPC read whose maximum accepts V1"
    };
    r.summary = format!("Bound decoder: {package} {version}; V1 is in scope from {reason}.");
    if supported == Some(true) {
        r.outcome = Outcome::Pass;
        r.title = "Resolved decoder has reviewed Transaction V1 read support".into();
    } else if supported == Some(false) {
        r.outcome = Outcome::Finding;
        r.severity = Some(Severity::Error);
        r.title = "Resolved decoder lacks reviewed Transaction V1 read support".into();
        r.remediation=Some(Remediation{summary:"Upgrade the bound decoder to a reviewed V1-capable release.".into(),location:client.manifest.clone(),steps:vec!["For the web3.js 1.x line, use @solana/web3.js 1.99.0 or a reviewed later 1.x release; for an explicit migration, @solana/kit 8.x is reviewed for V1 reads and sends.".into(),"Regenerate the lockfile with the project's package manager and rerun SolCompat.".into()],example:None,verification:vec!["Run client tests using known legacy, V0, and V1 responses.".into()]});
    } else {
        r.summary =
            format!("{package} {version} has no reviewed decoder record for Transaction V1.");
    }
    r
}
