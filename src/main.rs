mod experience;
mod render;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use solcompat_core::{
    check, AnalysisTarget, CheckResult, Counts, Dataset, DatasetIdentity, Outcome, Policy, Report,
    Severity,
};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "solcompat",
    version,
    about = "Check whether a Solana application is ready for ecosystem changes"
)]
struct Cli {
    /// Control terminal colors: auto uses colors only when stdout is a terminal.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: ColorMode,
    /// Run the detected program's normal SBF build and include the result.
    #[arg(long, global = true)]
    build: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Discover programs, clients, lock evidence, IDLs, artifacts, and RPC reads.
    Inspect(Common),
    /// Check the current project for reviewed Solana compatibility changes.
    Check(CheckArgs),
    /// Show breaking changes and migration actions for selected release targets.
    Upgrade(UpgradeArgs),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Format {
    #[default]
    Terminal,
    Json,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    fn enabled(self) -> bool {
        use std::io::IsTerminal;

        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => {
                io::stdout().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none()
                    && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
            }
        }
    }
}

#[derive(Args)]
struct Common {
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Explicit config file; otherwise read solcompat.toml inside --path when present.
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "terminal")]
    format: Format,
    /// Select local, a dated snapshot, or a conditional deployment scenario.
    #[arg(long, conflicts_with = "target_file")]
    target: Option<String>,
    /// Select an explicit local JSON target/scenario document.
    #[arg(long, conflicts_with = "target")]
    target_file: Option<PathBuf>,
    /// Validate and use an explicit local dataset instead of the bundled revision.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Import a captured `cargo metadata --format-version 1` document.
    #[arg(long)]
    cargo_metadata: Option<PathBuf>,
    /// Inspect an existing ELF artifact. The path must be inside --path.
    #[arg(long = "artifact")]
    artifacts: Vec<PathBuf>,
    /// Run bounded `--version` probes for installed tools.
    #[arg(long)]
    probe_tools: bool,
}

impl Default for Common {
    fn default() -> Self {
        Self {
            path: PathBuf::from("."),
            config: None,
            format: Format::default(),
            target: None,
            target_file: None,
            data: None,
            cargo_metadata: None,
            artifacts: Vec::new(),
            probe_tools: false,
        }
    }
}

#[derive(Args, Default)]
struct CheckArgs {
    #[command(flatten)]
    common: Common,
    /// Expand evidence, explanations, migration steps, and references.
    #[arg(long)]
    detailed: bool,
    #[arg(long)]
    deny_warnings: bool,
    #[arg(long)]
    deny_unknown: bool,
}

#[derive(Args)]
struct UpgradeArgs {
    #[command(flatten)]
    common: Common,
    /// Anchor release to assess as the migration target.
    #[arg(long, default_value = "1.2.0")]
    anchor: String,
    /// Agave release to assess as the deployment/tooling target.
    #[arg(long, default_value = "4.3.0")]
    agave: String,
    /// Expand evidence, migration steps, and references.
    #[arg(long)]
    detailed: bool,
}

#[derive(Serialize)]
struct ErrorReport {
    schema_version: u32,
    command: &'static str,
    exit_code: u8,
    error: ErrorDiagnostic,
}

#[derive(Serialize)]
struct ErrorDiagnostic {
    kind: &'static str,
    message: String,
}

fn load_data(common: &Common) -> Result<Dataset> {
    match &common.data {
        Some(path) => Dataset::parse(&solcompat_project::read_text(path)?),
        None => Dataset::bundled(),
    }
}

fn collect(common: &Common) -> Result<solcompat_project::Collected> {
    let mut collected = solcompat_project::collect(
        &common.path,
        common.config.as_deref(),
        if common.target_file.is_some() {
            Some("local")
        } else {
            common.target.as_deref()
        },
        common.cargo_metadata.as_deref(),
        &common.artifacts,
    )?;
    if common.probe_tools {
        collected.project.tools = solcompat_project::probe_tools();
        let anchor = collected
            .project
            .tools
            .iter()
            .find(|tool| tool.tool == "anchor")
            .and_then(|tool| tool.version.clone());
        let builder = collected
            .project
            .tools
            .iter()
            .find(|tool| tool.tool == "cargo-build-sbf")
            .and_then(|tool| tool.version.clone());
        for program in &mut collected.project.programs {
            if program.anchor_cli_version.is_none() {
                program.anchor_cli_version = anchor.clone();
            }
            if program.cargo_build_sbf_version.is_none() {
                program.cargo_build_sbf_version = builder.clone();
            }
        }
    }
    Ok(collected)
}

fn analyze(common: &Common, options: Option<&CheckArgs>) -> Result<Report> {
    let data = load_data(common)?;
    let collected = collect(common)?;
    let target = match &common.target_file {
        Some(path) => AnalysisTarget::parse(&solcompat_project::read_text(path)?)?,
        None => AnalysisTarget::named(&collected.target)?,
    };
    if let Some(options) = options {
        check(
            collected.project,
            &data,
            Policy {
                deny_warnings: options.deny_warnings,
                deny_unknown: options.deny_unknown,
            },
            &collected.suppressions,
            &target,
        )
    } else {
        Ok(Report {
            schema_version: 1,
            command: "inspect".into(),
            analysis_mode: "inventory".into(),
            target: target.id,
            target_digest: target.digest,
            dataset: DatasetIdentity {
                revision: data.revision,
                digest: data.digest,
            },
            project: collected.project,
            results: vec![],
            counts: Counts::default(),
            policy: Policy::default(),
            exit_code: 0,
        })
    }
}

fn analyze_upgrade(options: &UpgradeArgs) -> Result<Report> {
    let data = load_data(&options.common)?;
    let collected = collect(&options.common)?;
    experience::upgrade_report(collected.project, &data, &options.anchor, &options.agave)
}

fn append_build(report: &mut Report, root: &Path) -> Result<()> {
    if report.project.programs.len() != 1 {
        bail!(
            "--build requires exactly one detected program; found {}. Use --path to select one program project.",
            report.project.programs.len()
        );
    }
    let program = &report.project.programs[0];
    let observation = solcompat_project::build_program(root, program)?;
    let summary = if observation.success {
        format!("`{}` completed successfully.", observation.command)
    } else {
        format!("`{}` exited unsuccessfully.", observation.command)
    };
    report.results.push(CheckResult {
        rule_id: "SC100".into(),
        rule_name: "SBF program build".into(),
        rule_revision: 1,
        subject: program.id.clone(),
        operation: "build".into(),
        conditional: false,
        outcome: if observation.success {
            Outcome::Pass
        } else {
            Outcome::Finding
        },
        severity: (!observation.success).then_some(Severity::Error),
        title: if observation.success {
            "Program builds with the selected SBF toolchain".into()
        } else {
            "Program failed to build with the selected SBF toolchain".into()
        },
        summary,
        explanation: if observation.output.is_empty() {
            "The explicitly requested build produced no captured output.".into()
        } else {
            format!(
                "Last build output: {}",
                observation.output.replace('\n', " | ")
            )
        },
        evidence: program.evidence.clone(),
        sources: vec![],
        remediation: (!observation.success).then(|| solcompat_core::Remediation {
            summary: "Fix the reported SBF build error, then rerun `solcompat --build`.".into(),
            location: observation.manifest,
            steps: vec![],
            example: Some(observation.command),
            verification: vec!["Confirm SC100 passes with the intended release toolchain.".into()],
        }),
        suppression: None,
    });
    report.finish();
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let defaults = CheckArgs::default();
    let (common, detailed, command, mut result) = match &cli.command {
        Some(Command::Inspect(common)) => (
            common,
            false,
            "inspect",
            if cli.build {
                Err(anyhow::anyhow!(
                    "--build is available with the default check or `solcompat check`"
                ))
            } else {
                analyze(common, None)
            },
        ),
        Some(Command::Check(options)) => (
            &options.common,
            options.detailed,
            "check",
            analyze(&options.common, Some(options)),
        ),
        Some(Command::Upgrade(options)) => (
            &options.common,
            options.detailed,
            "upgrade",
            if cli.build {
                Err(anyhow::anyhow!(
                    "run `solcompat --build` separately from `solcompat upgrade`"
                ))
            } else {
                analyze_upgrade(options)
            },
        ),
        None => (
            &defaults.common,
            false,
            "check",
            analyze(&defaults.common, Some(&defaults)),
        ),
    };
    if cli.build {
        if let Ok(report) = &mut result {
            if let Err(error) = append_build(report, &common.path) {
                result = Err(error);
            }
        }
    }
    let (output, exit_code) = match result {
        Ok(report) => {
            let output = match common.format {
                Format::Json => serde_json::to_string_pretty(&report).expect("report serializes"),
                Format::Terminal => render::report(
                    &report,
                    detailed,
                    &render::detailed_hint(std::env::args_os()),
                    cli.color.enabled(),
                ),
            };
            (output, report.exit_code)
        }
        Err(error) => {
            let message = format!("{error:#}");
            match common.format {
                Format::Json => (
                    serde_json::to_string_pretty(&ErrorReport {
                        schema_version: 1,
                        command,
                        exit_code: 2,
                        error: ErrorDiagnostic {
                            kind: "input_or_analysis_error",
                            message,
                        },
                    })
                    .expect("error serializes"),
                    2,
                ),
                Format::Terminal => {
                    eprintln!("{}", render::error(&message, cli.color.enabled()));
                    return ExitCode::from(2);
                }
            }
        }
    };
    if let Err(error) = writeln!(io::stdout().lock(), "{output}") {
        if error.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("ERROR — cannot write report: {error}");
        }
        return ExitCode::from(2);
    }
    ExitCode::from(exit_code)
}
