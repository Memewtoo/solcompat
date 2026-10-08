use solcompat_core::{Outcome, Report, Severity, Verdict};
use std::{ffi::OsString, fmt::Write};

const RESET: &str = "\x1b[0m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_YELLOW: &str = "\x1b[1;33m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_MAGENTA: &str = "\x1b[1;35m";
const DIM: &str = "\x1b[2m";

fn paint(value: &str, color: &str, enabled: bool) -> String {
    if enabled {
        format!("{color}{value}{RESET}")
    } else {
        value.into()
    }
}

pub fn error(message: &str, color: bool) -> String {
    format!(
        "{} — {message}",
        paint("ERROR", BOLD_RED, color),
        message = one_line(message)
    )
}

// Prevent untrusted manifest/config text from injecting terminal controls or fake rows.
fn one_line(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                c.escape_unicode().to_string().chars().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_=./:".contains(&b))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

pub fn detailed_hint(args: impl Iterator<Item = OsString>) -> String {
    let mut args: Vec<_> = args.map(|arg| quote(&arg.to_string_lossy())).collect();
    if args.len() == 1 {
        args.push("check".into());
    }
    if !args.iter().any(|arg| arg == "--detailed") {
        args.push("--detailed".into());
    }
    args.join(" ")
}

pub fn report(report: &Report, detailed: bool, hint: &str, color: bool) -> String {
    let mut out = String::new();
    let has_visible_results = report
        .results
        .iter()
        .any(|result| result.outcome != Outcome::NotApplicable);
    writeln!(
        out,
        "SolCompat — {} · {}",
        one_line(&report.project.name),
        match report.command.as_str() {
            "inspect" => "inventory",
            "upgrade" => "upgrade advisor",
            _ if report
                .results
                .iter()
                .any(|result| result.operation == "build") =>
            {
                "compatibility + build"
            }
            _ => "compatibility scan",
        }
    )
    .unwrap();
    writeln!(out, "Target: {}", one_line(&report.target)).unwrap();
    if report.command != "inspect" {
        let (verdict, verdict_color) = match report.verdict() {
            Verdict::Inventory => ("INVENTORY", DIM),
            Verdict::BreakingChanges => ("BREAKING CHANGES DETECTED", BOLD_RED),
            Verdict::UpgradeReview => ("UPGRADE REVIEW NEEDED", BOLD_YELLOW),
            Verdict::Incompatible => ("INCOMPATIBLE FOR CHECKED RULES", BOLD_RED),
            Verdict::InputPolicyFailure => ("FAILED POLICY — INPUT REQUIRED", BOLD_RED),
            Verdict::Incomplete => ("INCOMPLETE", BOLD_YELLOW),
            Verdict::WarningPolicyFailure => ("FAILED POLICY — WARNINGS DENIED", BOLD_RED),
            Verdict::Warnings => ("PASSED WITH WARNINGS", BOLD_YELLOW),
            Verdict::Notes => ("PASSED WITH NOTES", BOLD_CYAN),
            Verdict::Suppressions => ("REVIEW SUPPRESSIONS", BOLD_MAGENTA),
            Verdict::NoUpgradeAdvice => ("NO APPLICABLE UPGRADE ADVICE", DIM),
            Verdict::NoChecks => ("NO APPLICABLE CHECKS", DIM),
            Verdict::Compatible => ("COMPATIBLE FOR CHECKED RULES", BOLD_GREEN),
        };
        writeln!(out, "Verdict: {}", paint(verdict, verdict_color, color)).unwrap();
        writeln!(
            out,
            "Scope: {} · {} · {} · {}",
            counted(report.project.programs.len(), "program", "programs"),
            counted(report.project.clients.len(), "client", "clients"),
            counted(report.project.idls.len(), "IDL", "IDLs"),
            counted(report.project.artifacts.len(), "artifact", "artifacts")
        )
        .unwrap();
        if !has_visible_results {
            writeln!(
                out,
                "{}",
                if report.command == "upgrade" {
                    "No reviewed migration advice applies to the detected components and selected targets."
                } else {
                    "No configured checks apply to the detected components and selected target."
                }
            )
            .unwrap();
            if report
                .project
                .programs
                .iter()
                .any(|program| program.framework == "pinocchio")
            {
                writeln!(
                    out,
                    "Pinocchio checks require an explicit sBPFv3 build profile (SC103) or a supplied artifact with an applicable target (SC102). Builds and target/ outputs are not imported automatically in v0.1."
                )
                .unwrap();
            }
        }
        if report.counts.warnings > 0
            && report.counts.errors == 0
            && report.counts.unknown == 0
            && report.counts.skipped == 0
        {
            writeln!(
                out,
                "No incompatibility was detected; {} should be reviewed.",
                counted(
                    report.counts.warnings,
                    "advisory warning",
                    "advisory warnings"
                )
            )
            .unwrap();
        }
        if report.counts.unknown > 0 && report.counts.errors == 0 {
            writeln!(
                out,
                "No incompatibility was detected, but {} could not be completed.",
                counted(report.counts.unknown, "check", "checks")
            )
            .unwrap();
        }
    }
    if detailed {
        if let Some(digest) = &report.target_digest {
            writeln!(out, "Target data: {digest}").unwrap();
        }
    }
    if report.command == "inspect" {
        for client in &report.project.clients {
            writeln!(
                out,
                "\nCLIENT {} — {}",
                one_line(&client.id),
                one_line(&client.manifest)
            )
            .unwrap();
            for (package, requirement) in &client.dependencies {
                writeln!(
                    out,
                    "  {}: {} (declared)",
                    one_line(package),
                    one_line(requirement)
                )
                .unwrap();
            }
            for (package, version) in &client.resolved_dependencies {
                writeln!(
                    out,
                    "  {}: {} (resolved)",
                    one_line(package),
                    one_line(version)
                )
                .unwrap();
            }
            writeln!(out, "  RPC read contracts: {}", client.rpc_reads.len()).unwrap();
        }
        for program in &report.project.programs {
            writeln!(
                out,
                "\nPROGRAM CANDIDATE {} — {}",
                one_line(&program.id),
                one_line(&program.manifest)
            )
            .unwrap();
            writeln!(
                out,
                "  Framework: {} ({})",
                one_line(&program.framework),
                one_line(&program.candidate_reason)
            )
            .unwrap();
            for (package, version) in &program.resolved_dependencies {
                writeln!(
                    out,
                    "  {}: {} (resolved)",
                    one_line(package),
                    one_line(version)
                )
                .unwrap();
            }
        }
        for idl in &report.project.idls {
            writeln!(
                out,
                "\nIDL {} — {} ({})",
                one_line(&idl.id),
                one_line(&idl.path),
                one_line(&idl.schema)
            )
            .unwrap();
        }
        for artifact in &report.project.artifacts {
            writeln!(
                out,
                "\nARTIFACT {} — {} (sBPF{})",
                one_line(&artifact.id),
                one_line(&artifact.path),
                one_line(artifact.sbpf_version.as_deref().unwrap_or("unknown"))
            )
            .unwrap();
        }
        for tool in &report.project.tools {
            writeln!(
                out,
                "\nTOOL {} — {}",
                one_line(&tool.tool),
                one_line(tool.version.as_deref().unwrap_or(&tool.status))
            )
            .unwrap();
        }
        writeln!(
            out,
            "\n{} component(s) inspected; no compatibility checks executed.",
            report.project.clients.len() + report.project.programs.len()
        )
        .unwrap();
        return out.trim_end().into();
    }
    if detailed {
        writeln!(
            out,
            "Data: {} ({})",
            one_line(&report.dataset.revision),
            report.dataset.digest
        )
        .unwrap();
    }
    if detailed && has_visible_results {
        writeln!(
            out,
            "Status: {} verified · {} incompatible · {} review · {} insufficient evidence",
            paint("PASS", BOLD_GREEN, color),
            paint("ERROR", BOLD_RED, color),
            paint("WARNING", BOLD_YELLOW, color),
            paint("NEEDS INPUT", BOLD_YELLOW, color)
        )
        .unwrap();
    }
    if has_visible_results {
        out.push('\n');
    }
    for result in &report.results {
        // Not-applicable results remain in JSON for auditing, but do not help a
        // developer act on a terminal report—even in detailed mode.
        if result.outcome == Outcome::NotApplicable {
            continue;
        }
        let (status, status_color) = if result.suppression.is_some() {
            ("SUPPRESSED", BOLD_MAGENTA)
        } else {
            match result.outcome {
                Outcome::Pass => ("PASS", BOLD_GREEN),
                Outcome::Unknown => ("NEEDS INPUT", BOLD_YELLOW),
                Outcome::NotApplicable => ("N/A", DIM),
                Outcome::Skipped => ("SKIPPED", BOLD_CYAN),
                Outcome::Finding => match result.severity {
                    Some(Severity::Error) if report.command == "upgrade" => ("BREAKING", BOLD_RED),
                    Some(Severity::Error) => ("ERROR", BOLD_RED),
                    Some(Severity::Warning) => ("WARNING", BOLD_YELLOW),
                    _ => ("INFO", BOLD_CYAN),
                },
            }
        };
        let title = if detailed {
            &result.title
        } else {
            &result.rule_name
        };
        writeln!(
            out,
            "{}{} — {} ({}) · {}",
            paint(status, status_color, color),
            if result.conditional {
                " [CONDITIONAL]"
            } else {
                ""
            },
            one_line(title),
            result.rule_id,
            one_line(&result.subject)
        )
        .unwrap();
        writeln!(out, "  {}", one_line(&result.summary)).unwrap();
        if let Some(suppression) = &result.suppression {
            writeln!(out, "  Suppressed: {}", one_line(&suppression.reason)).unwrap();
        }
        if detailed {
            writeln!(out, "  Why: {}", one_line(&result.explanation)).unwrap();
            for evidence in &result.evidence {
                writeln!(
                    out,
                    "  Evidence: {} [{}; {}]",
                    one_line(&evidence.path),
                    one_line(&evidence.pointer),
                    one_line(&evidence.kind)
                )
                .unwrap();
            }
            if let Some(fix) = &result.remediation {
                writeln!(
                    out,
                    "  Next: {}\n  Where: {}",
                    one_line(&fix.summary),
                    one_line(&fix.location)
                )
                .unwrap();
                for (index, step) in fix.steps.iter().enumerate() {
                    writeln!(out, "  {}. {}", index + 1, one_line(step)).unwrap();
                }
                if let Some(example) = &fix.example {
                    writeln!(out, "  Example:").unwrap();
                    for line in example.lines() {
                        writeln!(out, "    {}", one_line(line)).unwrap();
                    }
                }
                for step in &fix.verification {
                    writeln!(out, "  Verify: {}", one_line(step)).unwrap();
                }
            }
            for source in &result.sources {
                writeln!(out, "  Reference: {}", one_line(source)).unwrap();
            }
        } else if let Some(fix) = &result.remediation {
            writeln!(out, "  Next: {}", one_line(&fix.summary)).unwrap();
        }
        out.push('\n');
    }
    let c = &report.counts;
    let counts = [
        (c.passed, "passed", "passed"),
        (c.errors, "error", "errors"),
        (c.warnings, "warning", "warnings"),
        (c.info, "info", "info"),
        (c.unknown, "check needs input", "checks need input"),
        (c.skipped, "skipped", "skipped"),
        (c.suppressed, "suppressed", "suppressed"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, singular, plural)| {
        format!("{count} {}", if count == 1 { singular } else { plural })
    })
    .collect::<Vec<_>>()
    .join(" · ");
    if !counts.is_empty() {
        writeln!(out, "{counts}").unwrap();
    }
    writeln!(out, "Exit code: {}", report.exit_code).unwrap();
    if !detailed && has_visible_results {
        writeln!(out, "Details: {}", one_line(hint)).unwrap();
    }
    out.trim_end().into()
}
