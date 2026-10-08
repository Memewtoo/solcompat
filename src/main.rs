mod app;
mod cli;
mod render;

use app::{analyze, analyze_upgrade, append_build};
use clap::Parser;
use cli::{CheckArgs, Cli, Command, Format};
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
};

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
            let options = match &cli.command {
                Some(Command::Check(options)) => options,
                _ => &defaults,
            };
            if let Err(error) = append_build(
                report,
                common,
                options,
                std::time::Duration::from_secs(cli.build_timeout_seconds.unwrap_or(1800)),
            ) {
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
