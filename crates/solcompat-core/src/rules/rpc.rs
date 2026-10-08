//! SC201: request acceptance from effective RPC options and required read versions; incomplete contracts remain unknown.

use crate::evaluation::advice;
use crate::{CheckResult, Client, Dataset, Outcome, RpcMaximum, RpcRead, Severity};

pub(super) fn base(client: &Client, read: &RpcRead, data: &Dataset) -> CheckResult {
    let record = data.rpc_record();
    CheckResult {
        rule_id: record.rule_id.clone(),
        rule_name: record.rule_name.clone(),
        rule_revision: record.revision,
        subject: format!("{}/{}", client.id, read.id),
        operation: read.method.clone(),
        conditional: false,
        outcome: Outcome::Unknown,
        severity: None,
        title: "RPC transaction read needs additional evidence".into(),
        summary: "The configured RPC read is missing information required for evaluation.".into(),
        explanation: "SC201 evaluates an explicitly configured getBlock or getTransaction read. Missing request options or transaction-version requirements remain unknown and do not claim an incompatibility.".into(),
        evidence: client.evidence.iter().filter(|ev| {
            if matches!(ev.kind.as_str(), "observed-rpc" | "unresolved-rpc") { ev.pointer.ends_with(&format!("/read:{}", read.id)) }
            else { ev.kind != "resolution" && ev.kind != "metadata-resolution" }
        }).cloned().collect(),
        sources: record.sources.clone(),
        remediation: Some(advice("Complete this RPC read contract in solcompat.toml.")),
        suppression: None,
    }
}

pub(super) fn unknown(result: &mut CheckResult, summary: &str, action: &str) {
    result.summary = summary.into();
    result.remediation = Some(advice(action));
}

pub(super) fn evaluate(client: &Client, read: &RpcRead, data: &Dataset) -> CheckResult {
    let mut result = base(client, read, data);
    let record = data.rpc_record();
    if result.evidence.iter().any(|ev| ev.kind == "unresolved-rpc") {
        unknown(&mut result, "A matching method call was found, but its receiver could not be bound to a supported Solana SDK.", "Declare this read and its decoder in solcompat.toml; source discovery does not resolve wrappers, parameters, aliases, or shadowed variables.");
        return result;
    }
    // An explicit empty requirement declares no transaction-read capability to check.
    if client
        .required_read_versions
        .as_ref()
        .is_some_and(Vec::is_empty)
    {
        result.outcome = Outcome::NotApplicable;
        result.title = "No transaction-read requirement declared".into();
        result.summary = "The client explicitly declares no required transaction formats.".into();
        result.remediation = None;
        return result;
    }
    if !record.methods.contains(&read.method) {
        unknown(
            &mut result,
            "This method has no reviewed RPC acceptance record.",
            "Select a dataset with a reviewed record for this method.",
        );
        return result;
    }
    if read.method == "getBlock" {
        match read.transaction_details.as_deref() {
            Some(details)
                if record
                    .body_free_block_details
                    .iter()
                    .any(|value| value == details) =>
            {
                result.outcome = Outcome::NotApplicable;
                result.title = "Block read does not return transaction bodies".into();
                result.summary = format!("transactionDetails={details}; the transaction-body version limit is not applicable.");
                result.remediation = None;
                return result;
            }
            Some("full") => {}
            Some(_) => {
                unknown(&mut result, "This block response mode is not reviewed in the selected dataset.", "Use a reviewed response-mode record; do not infer full-transaction requirements for this mode.");
                return result;
            }
            None => {
                unknown(
                    &mut result,
                    "The getBlock transaction_details option is unknown.",
                    "Declare the effective transaction_details value used by this read.",
                );
                return result;
            }
        }
    }
    let Some(required) = &client.required_read_versions else {
        return result;
    };
    if required
        .iter()
        .any(|version| !record.reviewed_versions.contains(version))
    {
        unknown(&mut result, "A required transaction format is outside the reviewed dataset.", "Use a reviewed compatibility record for the required format; do not assume future version support.");
        return result;
    }
    let Some(encoding) = &read.encoding else {
        unknown(
            &mut result,
            "The request encoding is unknown.",
            "Declare the effective encoding used by this RPC read.",
        );
        return result;
    };
    if !record.reviewed_encodings.contains(encoding) {
        unknown(
            &mut result,
            "The request encoding is outside the reviewed dataset.",
            "Select a reviewed record for this encoding.",
        );
        return result;
    }
    let Some(maximum) = &read.max_supported_transaction_version else {
        unknown(&mut result, "The request maximum is unknown; absence of a declaration is not RPC omission.", "Declare the effective maximum, or use the explicit string \"omitted\" if the application omits the RPC option.");
        return result;
    };
    if let RpcMaximum::Version(version) = maximum {
        if !record.reviewed_versions.contains(&format!("v{version}")) {
            unknown(&mut result, "The configured maximum is outside the reviewed dataset.", "Use a reviewed record for this maximum; increasing the number alone does not establish support.");
            return result;
        }
    }
    let highest = required
        .iter()
        .filter_map(|version| version.strip_prefix('v')?.parse::<u8>().ok())
        .max();
    let supported = match (highest, maximum) {
        (None, _) => true,
        (Some(required), RpcMaximum::Version(configured)) => *configured >= required,
        (Some(_), RpcMaximum::Omitted(_)) => false,
    };
    let required_text = highest.map_or_else(|| "legacy".into(), |v| format!("v{v}"));
    let configured = match maximum {
        RpcMaximum::Version(value) => value.to_string(),
        RpcMaximum::Omitted(_) => "omitted (legacy only)".into(),
    };
    result.summary =
        format!("Declared maximum: {configured}; highest required format: {required_text}.");
    if supported {
        result.outcome = Outcome::Pass;
        result.title = "RPC limit accepts the declared transaction versions".into();
        result.explanation = "The declared request limit meets the declared version requirement. This does not establish decoder support or prove the running application uses these options.".into();
        result.remediation = None;
    } else {
        let needed = highest.expect("an unsupported requirement is versioned");
        result.outcome = Outcome::Finding;
        result.severity = Some(Severity::Error);
        result.title = format!("RPC read limit excludes required Transaction V{needed}");
        result.explanation = if read.method == "getBlock" {
            "A full-block read containing a transaction above this maximum can fail as a whole. The configured limit does not filter out newer transactions.".into()
        } else {
            "Reading a transaction above this maximum can fail even if the transaction succeeded on-chain.".into()
        };
        let mut fix = advice(format!("Set maxSupportedTransactionVersion: {needed} in the actual {} request options, after ensuring decoder support.", read.method));
        let source_location = result.evidence.iter().find_map(|evidence| {
            evidence.pointer.strip_prefix("/source:").map(|line| {
                format!(
                    "{} at line {}",
                    evidence.path,
                    line.split('/').next().unwrap_or(line)
                )
            })
        });
        fix.location = source_location.clone().unwrap_or_else(|| {
            "The bound client's RPC read options or shared request wrapper; application source location was not detected.".into()
        });
        fix.steps = vec![
            format!("Ensure the bound decoder supports Transaction V{needed}; SC201 checks request acceptance only."),
            format!("Set maxSupportedTransactionVersion to the integer {needed} in this client's {} options or shared wrapper. Preserve its other options.", read.method),
            if source_location.is_some() {
                "Rerun SolCompat to verify the changed source call and decoder version.".into()
            } else {
                "Update the SolCompat declaration to match the changed application request. Editing solcompat.toml alone does not fix the application.".into()
            },
        ];
        fix.example = Some(format!("// Example option to merge into the application's request configuration:\n{{ maxSupportedTransactionVersion: {needed} }}"));
        fix.verification = vec![
            format!("Run the client's typecheck and tests against a known V{needed} response and the other required formats."),
            "Rerun solcompat check with the same input selection. These developer verification steps are not executed by SolCompat.".into(),
        ];
        result.remediation = Some(fix);
    }
    result
}
