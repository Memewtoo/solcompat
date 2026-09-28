use crate::*;
use anyhow::{ensure, Result};
use semver::Version;

fn advice(summary: impl Into<String>) -> Remediation {
    Remediation {
        summary: summary.into(),
        location: "The bound component configuration or manifest; an application source location was not inferred.".into(),
        steps: vec![],
        example: None,
        verification: vec![],
    }
}
fn advice_at(summary: impl Into<String>, location: impl Into<String>) -> Remediation {
    let mut remediation = advice(summary);
    remediation.location = location.into();
    remediation
}

fn base(client: &Client, read: &RpcRead, data: &Dataset) -> CheckResult {
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
        evidence: client.evidence.clone(),
        sources: record.sources.clone(),
        remediation: Some(advice("Complete this RPC read contract in solcompat.toml.")),
        suppression: None,
    }
}

fn unknown(result: &mut CheckResult, summary: &str, action: &str) {
    result.summary = summary.into();
    result.remediation = Some(advice(action));
}

fn evaluate(client: &Client, read: &RpcRead, data: &Dataset) -> CheckResult {
    let mut result = base(client, read, data);
    let record = data.rpc_record();
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
        let source_location = client.evidence.iter().find_map(|evidence| {
            evidence
                .pointer
                .strip_prefix("/source:")
                .map(|line| format!("{} at line {line}", evidence.path))
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

fn rule_result(
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

fn parsed(value: Option<&String>) -> Option<Version> {
    parse_version(value?)
}
fn parse_version(value: &str) -> Option<Version> {
    let value = value.trim_start_matches('v');
    Version::parse(value)
        .ok()
        .or_else(|| Version::parse(&format!("{value}.0")).ok())
}
fn resolved(program: &Program, name: &str) -> Option<Version> {
    parsed(program.resolved_dependencies.get(name))
}
fn declares(program: &Program, name: &str) -> bool {
    program.dependencies.keys().any(|key| {
        key.rsplit('/')
            .next()
            .is_some_and(|identity| identity == name || identity.ends_with(&format!("=>{name}")))
    })
}

fn declared_requirement<'a>(program: &'a Program, name: &str) -> Option<&'a str> {
    program.dependencies.iter().find_map(|(key, requirement)| {
        key.rsplit('/').next().and_then(|identity| {
            (identity == name || identity.ends_with(&format!("=>{name}")))
                .then_some(requirement.as_str())
        })
    })
}

fn program_base_evidence(program: &Program) -> Vec<Evidence> {
    program
        .evidence
        .iter()
        .filter(|evidence| {
            !evidence.kind.starts_with("pinocchio-") && !evidence.kind.starts_with("anchor-")
        })
        .cloned()
        .collect()
}

fn program_results(program: &Program, data: &Dataset) -> Vec<CheckResult> {
    let mut out = Vec::new();
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
    out.push(rust);

    let mut cli = rule_result(
        data,
        "SC003",
        &program.id,
        "Anchor build/IDL generation",
        program_base_evidence(program),
    );
    if program.framework != "anchor" {
        cli.outcome = Outcome::NotApplicable;
        cli.title = "Program is not identified as Anchor".into();
        cli.summary = format!("Detected framework: {}.", program.framework);
    } else {
        match (
            parsed(program.anchor_cli_version.as_ref()),
            resolved(program, "anchor-lang"),
        ) {
            (Some(a), Some(b)) if (a.major, a.minor) == (b.major, b.minor) => {
                cli.outcome = Outcome::Pass;
                cli.title = "Anchor CLI and anchor-lang release lines align".into();
                cli.summary = format!("Anchor CLI {a}; anchor-lang {b}.");
            }
            (Some(a), Some(b)) => {
                cli.outcome = Outcome::Finding;
                cli.severity = Some(Severity::Warning);
                cli.title = "Anchor CLI differs from Cargo-resolved anchor-lang".into();
                cli.summary = match declared_requirement(program, "anchor-lang") {
                    Some(requirement) => format!(
                        "Cargo accepted anchor-lang {b} for requirement {requirement}; selected Anchor CLI is {a}."
                    ),
                    None => format!("Anchor CLI {a}; resolved anchor-lang {b}."),
                };
                cli.explanation = "The resolved crate satisfies Cargo's declared version range. This establishes dependency-range compatibility, but it does not prove that the selected CLI and resolved framework release are fully aligned or that current source built successfully. Anchor release guidance updates the CLI and framework crates together, so the drift remains advisory.".into();
                let mut remediation = advice_at(
                    "Consider aligning the selected CLI and resolved Anchor packages; keep the current combination if its successful build and generated outputs are intentionally accepted.",
                    &program.manifest,
                );
                remediation.steps = vec![
                    format!("To keep resolved anchor-lang {b}, select Anchor CLI {b} and align the other Anchor packages to that release."),
                    format!("To keep Anchor CLI {a}, intentionally constrain anchor-lang to the intended {a} release, then update Cargo.lock. Use an exact requirement only when strict pinning is intended."),
                    "Run the normal Anchor build, review the regenerated IDL/client output, and rerun SolCompat.".into(),
                ];
                remediation.verification = vec![
                    "Confirm SC003 reports aligned release lines and the project build/tests pass with the selected versions.".into(),
                ];
                cli.remediation = Some(remediation);
            }
            (Some(anchor_cli), None) => {
                cli.title = "Resolved anchor-lang version is unavailable".into();
                cli.summary = match declared_requirement(program, "anchor-lang") {
                    Some(requirement) => format!(
                        "Anchor CLI {anchor_cli} is selected; anchor-lang requirement {requirement} is declared but not resolved."
                    ),
                    None => format!(
                        "Anchor CLI {anchor_cli} is selected; no resolved anchor-lang version is available."
                    ),
                };
                let mut remediation = advice_at(
                    "Resolve the program dependencies, then rerun SolCompat with Cargo.lock or captured cargo metadata.",
                    &program.manifest,
                );
                remediation.steps = vec![
                    "If Cargo.lock is absent, run anchor build for the normal project build, or run cargo generate-lockfile when only dependency resolution is needed.".into(),
                    "Rerun solcompat check --detailed; SolCompat will use an unambiguous crates.io version from Cargo.lock.".into(),
                    "If Cargo.lock contains multiple anchor-lang versions, capture cargo metadata --format-version 1 and pass the JSON file with --cargo-metadata.".into(),
                ];
                remediation.example = Some(
                    "# Full Anchor build:\nanchor build\n\n# Or dependency resolution only:\ncargo generate-lockfile\n\nsolcompat check --detailed"
                        .into(),
                );
                remediation.verification = vec![
                    "Confirm SC003 reports the selected Anchor CLI and the resolved anchor-lang version.".into(),
                ];
                cli.remediation = Some(remediation);
            }
            (None, Some(anchor_lang)) => {
                cli.title = "Anchor CLI version is unavailable".into();
                cli.summary = format!(
                    "anchor-lang {anchor_lang} is resolved; no Anchor CLI version is selected or observed."
                );
                cli.remediation = Some(advice_at(
                    "Set [toolchain].anchor_version in Anchor.toml, configure anchor_cli_version for this program, or use --probe-tools with an installed native Anchor executable.",
                    &program.manifest,
                ));
            }
            (None, None) => {
                cli.title = "Anchor CLI and resolved anchor-lang versions are unavailable".into();
                cli.summary = "SC003 requires a selected or observed Anchor CLI version and a resolved anchor-lang version.".into();
                cli.remediation = Some(advice_at(
                    "Supply the Anchor CLI version and Cargo.lock or captured cargo metadata resolving anchor-lang.",
                    &program.manifest,
                ));
            }
        }
    }
    out.push(cli);

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
    out.push(arch);

    let mut crates = rule_result(
        data,
        "SC302",
        &program.id,
        "compile Anchor program",
        program_base_evidence(program),
    );
    if program.framework != "anchor" {
        crates.outcome = Outcome::NotApplicable;
        crates.title = "Program is not identified as Anchor".into();
        crates.summary = format!("Detected framework: {}.", program.framework);
    } else {
        match (
            resolved(program, "anchor-lang"),
            resolved(program, "anchor-spl"),
        ) {
            (Some(a), Some(b)) if (a.major, a.minor) == (b.major, b.minor) => {
                crates.outcome = Outcome::Pass;
                crates.title = "Anchor crate release lines align".into();
                crates.summary = format!("anchor-lang {a}; anchor-spl {b}.");
            }
            (Some(a), Some(b)) => {
                crates.outcome = Outcome::Finding;
                crates.severity = Some(Severity::Warning);
                crates.title = "Anchor crate release lines differ".into();
                crates.summary = format!("anchor-lang {a}; anchor-spl {b}.");
                crates.remediation=Some(advice_at("Align anchor-lang and anchor-spl to the same reviewed release line unless the chosen combination is explicitly supported.", &program.manifest));
            }
            (_, None) => {
                crates.outcome = Outcome::NotApplicable;
                crates.title = "anchor-spl is not a resolved dependency".into();
                crates.summary = "No anchor-spl relationship is present to compare.".into();
            }
            _ => {
                crates.summary =
                    "Resolved Anchor crate versions are required for comparison.".into();
            }
        }
    }
    out.push(crates);
    out
}

fn declared_npm_major_cap(requirement: &str, boundary: u64) -> bool {
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

fn has_source_signal(program: &Program, signal: &str) -> bool {
    program
        .evidence
        .iter()
        .any(|evidence| evidence.kind == signal)
}

fn framework_syntax_result(program: &Program, data: &Dataset) -> CheckResult {
    let mut evidence = program
        .evidence
        .iter()
        .filter(|evidence| evidence.kind.starts_with("pinocchio-"))
        .cloned()
        .collect::<Vec<_>>();
    evidence.sort_by(|a, b| (&a.path, &a.pointer).cmp(&(&b.path, &b.pointer)));
    evidence.dedup_by(|a, b| a.path == b.path && a.pointer == b.pointer);
    if evidence.is_empty() {
        evidence = program_base_evidence(program);
    }
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

    let legacy_types = has_source_signal(program, "pinocchio-account-info")
        || has_source_signal(program, "pinocchio-pubkey");
    let immutable_entrypoint = has_source_signal(program, "pinocchio-immutable-entrypoint");
    let mutable_entrypoint = has_source_signal(program, "pinocchio-mutable-entrypoint");
    let source_location = result.evidence.first().map_or_else(
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
        result.title = "Pinocchio entrypoint syntax matches the resolved 0.11+ API".into();
        result.summary = format!(
            "Resolved Pinocchio {current}; process_instruction accepts &mut [AccountView], and no legacy Pinocchio type imports were observed."
        );
        result.remediation = None;
    }
    result
}

fn decoder_result(client: &Client, data: &Dataset) -> CheckResult {
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

fn artifact_result(a: &Artifact, data: &Dataset, target: &AnalysisTarget) -> CheckResult {
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

fn idl_result(idl: &IdlInput, project: &Project, data: &Dataset) -> CheckResult {
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

pub fn check(
    project: Project,
    data: &Dataset,
    policy: Policy,
    suppressions: &[Suppression],
    target: &AnalysisTarget,
) -> Result<Report> {
    let mut results = vec![];
    for program in &project.programs {
        results.extend(program_results(program, data));
        results.push(framework_syntax_result(program, data));
    }
    for artifact in &project.artifacts {
        results.push(artifact_result(artifact, data, target));
    }
    for client in &project.clients {
        results.push(decoder_result(client, data));
        for read in &client.rpc_reads {
            results.push(evaluate(client, read, data));
        }
    }
    for idl in &project.idls {
        results.push(idl_result(idl, &project, data));
    }
    for suppression in suppressions {
        let result = results.iter_mut().find(|result| {
            result.rule_id == suppression.rule && result.subject == suppression.subject
        });
        let Some(result) = result else {
            anyhow::bail!(
                "suppression does not match an evaluated rule/subject: {} {}",
                suppression.rule,
                suppression.subject
            );
        };
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
    let mut report = Report {
        schema_version: 1,
        command: "check".into(),
        analysis_mode: "static".into(),
        target: target.id.clone(),
        target_digest: target.digest.clone(),
        dataset: DatasetIdentity {
            revision: data.revision.clone(),
            digest: data.digest.clone(),
        },
        project,
        results,
        counts: Counts::default(),
        policy,
        exit_code: 0,
    };
    report.finish();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn client(maximum: Option<RpcMaximum>) -> Client {
        Client {
            id: "indexer".into(),
            name: None,
            manifest: "package.json".into(),
            dependencies: BTreeMap::new(),
            resolved_dependencies: BTreeMap::new(),
            decoder_package: None,
            required_read_versions: Some(vec!["legacy".into(), "v0".into(), "v1".into()]),
            rpc_reads: vec![RpcRead {
                id: "blocks".into(),
                method: "getBlock".into(),
                encoding: Some("json".into()),
                transaction_details: Some("full".into()),
                max_supported_transaction_version: maximum,
            }],
            evidence: vec![],
        }
    }

    fn run(client: &Client) -> CheckResult {
        evaluate(
            client,
            client.rpc_reads.first().expect("test read"),
            &Dataset::bundled().unwrap(),
        )
    }

    #[test]
    fn acceptance_boundaries_and_omission_are_distinct() {
        for (maximum, expected) in [
            (Some(RpcMaximum::Version(0)), Outcome::Finding),
            (Some(RpcMaximum::Version(1)), Outcome::Pass),
            (
                Some(RpcMaximum::Omitted(Omitted::Explicit)),
                Outcome::Finding,
            ),
            (None, Outcome::Unknown),
            (Some(RpcMaximum::Version(2)), Outcome::Unknown),
        ] {
            assert_eq!(run(&client(maximum)).outcome, expected);
        }
        let mut legacy = client(Some(RpcMaximum::Omitted(Omitted::Explicit)));
        legacy.required_read_versions = Some(vec!["legacy".into()]);
        assert_eq!(run(&legacy).outcome, Outcome::Pass);
        legacy.required_read_versions = Some(vec!["v0".into()]);
        assert_eq!(run(&legacy).outcome, Outcome::Finding);
        legacy.rpc_reads[0].max_supported_transaction_version = Some(RpcMaximum::Version(0));
        assert_eq!(run(&legacy).outcome, Outcome::Pass);
    }

    #[test]
    fn response_modes_and_unknowns_do_not_create_false_failures() {
        let mut value = client(None);
        for details in ["none", "signatures"] {
            value.rpc_reads[0].transaction_details = Some(details.into());
            assert_eq!(run(&value).outcome, Outcome::NotApplicable);
        }
        value.rpc_reads[0].transaction_details = Some("accounts".into());
        assert_eq!(run(&value).outcome, Outcome::Unknown);
        value.rpc_reads[0].method = "getTransaction".into();
        value.rpc_reads[0].transaction_details = None;
        value.rpc_reads[0].max_supported_transaction_version = Some(RpcMaximum::Version(1));
        assert_eq!(run(&value).outcome, Outcome::Pass);
        value.required_read_versions = Some(vec!["v2".into()]);
        assert_eq!(run(&value).outcome, Outcome::Unknown);
        value.required_read_versions = None;
        assert_eq!(run(&value).outcome, Outcome::Unknown);
        value.required_read_versions = Some(vec![]);
        assert_eq!(run(&value).outcome, Outcome::NotApplicable);
    }

    fn program() -> Program {
        Program {
            id: "vault".into(),
            name: "vault".into(),
            manifest: "programs/vault/Cargo.toml".into(),
            framework: "anchor".into(),
            candidate_reason: "test".into(),
            rust_version: Some("1.79".into()),
            dependencies: BTreeMap::from([("dependencies/anchor-lang".into(), "0.31.1".into())]),
            resolved_dependencies: BTreeMap::from([
                ("anchor-lang".into(), "0.31.1".into()),
                ("anchor-spl".into(), "0.31.1".into()),
            ]),
            anchor_cli_version: Some("0.31.1".into()),
            sbf_rust_version: Some("1.79.0".into()),
            sbpf_arch: Some("v3".into()),
            platform_tools_version: Some("1.53.0".into()),
            cargo_build_sbf_version: Some("4.2.0".into()),
            evidence: vec![],
        }
    }

    fn outcome(results: &[CheckResult], id: &str) -> Outcome {
        results
            .iter()
            .find(|result| result.rule_id == id)
            .unwrap()
            .outcome
    }

    #[test]
    fn program_rules_preserve_pass_finding_unknown_and_applicability() {
        let data = Dataset::bundled().unwrap();
        let baseline = program();
        let results = program_results(&baseline, &data);
        for id in ["SC001", "SC003", "SC103", "SC302"] {
            assert_eq!(outcome(&results, id), Outcome::Pass, "{id}");
        }

        let mut changed = baseline.clone();
        changed.sbf_rust_version = Some("1.78.0".into());
        changed.anchor_cli_version = Some("0.30.1".into());
        changed.platform_tools_version = Some("1.52.0".into());
        changed
            .resolved_dependencies
            .insert("anchor-spl".into(), "0.30.1".into());
        let results = program_results(&changed, &data);
        for id in ["SC001", "SC003", "SC103", "SC302"] {
            assert_eq!(outcome(&results, id), Outcome::Finding, "{id}");
        }
        let sc003 = results
            .iter()
            .find(|result| result.rule_id == "SC003")
            .unwrap();
        assert!(sc003.summary.contains("Cargo accepted anchor-lang 0.31.1"));
        assert!(sc003.summary.contains("requirement 0.31.1"));
        assert!(sc003.explanation.contains("dependency-range compatibility"));

        let mut unresolved_anchor = baseline.clone();
        unresolved_anchor
            .resolved_dependencies
            .remove("anchor-lang");
        let results = program_results(&unresolved_anchor, &data);
        let sc003 = results
            .iter()
            .find(|result| result.rule_id == "SC003")
            .unwrap();
        assert_eq!(sc003.outcome, Outcome::Unknown);
        assert_eq!(sc003.title, "Resolved anchor-lang version is unavailable");
        assert!(sc003.summary.contains("Anchor CLI 0.31.1 is selected"));
        assert!(sc003.summary.contains("requirement 0.31.1 is declared"));
        assert!(!sc003
            .remediation
            .as_ref()
            .unwrap()
            .summary
            .contains("anchor_cli_version"));
        assert!(sc003.remediation.as_ref().unwrap().steps[0].contains("anchor build"));

        let mut missing = baseline.clone();
        missing.sbf_rust_version = None;
        missing.anchor_cli_version = None;
        missing.platform_tools_version = None;
        missing.resolved_dependencies.remove("anchor-lang");
        let results = program_results(&missing, &data);
        for id in ["SC001", "SC003", "SC103"] {
            assert_eq!(outcome(&results, id), Outcome::Unknown, "{id}");
        }

        let mut native = baseline;
        native.framework = "native".into();
        native.rust_version = None;
        native.sbpf_arch = None;
        let results = program_results(&native, &data);
        for id in ["SC001", "SC003", "SC103", "SC302"] {
            assert_eq!(outcome(&results, id), Outcome::NotApplicable, "{id}");
        }
    }

    #[test]
    fn decoder_and_idl_rules_require_exact_bound_evidence() {
        let data = Dataset::bundled().unwrap();
        let mut value = client(Some(RpcMaximum::Version(1)));
        value.decoder_package = Some("@solana/web3.js".into());
        value
            .resolved_dependencies
            .insert("@solana/web3.js".into(), "1.99.0".into());
        assert_eq!(decoder_result(&value, &data).outcome, Outcome::Pass);
        value
            .resolved_dependencies
            .insert("@solana/web3.js".into(), "1.98.4".into());
        assert_eq!(decoder_result(&value, &data).outcome, Outcome::Finding);
        value.resolved_dependencies.clear();
        assert_eq!(decoder_result(&value, &data).outcome, Outcome::Unknown);
        value.required_read_versions = Some(vec![]);
        assert_eq!(
            decoder_result(&value, &data).outcome,
            Outcome::NotApplicable
        );

        // An RPC maximum that accepts V1 independently places V1 decoding in
        // scope; the compatibility contract does not need to repeat it.
        value.required_read_versions = None;
        value
            .resolved_dependencies
            .insert("@solana/kit".into(), "8.0.0".into());
        value.decoder_package = Some("@solana/kit".into());
        assert_eq!(decoder_result(&value, &data).outcome, Outcome::Pass);

        value
            .resolved_dependencies
            .insert("@coral-xyz/anchor".into(), "0.30.1".into());
        let idl = IdlInput {
            id: "vault-idl".into(),
            path: "idl/vault.json".into(),
            client: "indexer".into(),
            reader_package: "@coral-xyz/anchor".into(),
            schema: "anchor-spec-0.1.0".into(),
            evidence: vec![],
        };
        let mut project = Project {
            name: "test".into(),
            clients: vec![value.clone()],
            programs: vec![],
            idls: vec![],
            artifacts: vec![],
            tools: vec![],
        };
        assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Pass);
        project.clients[0]
            .resolved_dependencies
            .insert("@coral-xyz/anchor".into(), "0.29.0".into());
        assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Finding);
        project.clients[0].resolved_dependencies.clear();
        assert_eq!(idl_result(&idl, &project, &data).outcome, Outcome::Unknown);
    }
}
