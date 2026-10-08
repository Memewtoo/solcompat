//! Command arguments and terminal-color preference.
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{io, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "solcompat",
    version,
    about = "Check whether a Solana application is ready for ecosystem changes"
)]
pub(crate) struct Cli {
    /// Control terminal colors: auto uses colors only when stdout is a terminal.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub(crate) color: ColorMode,
    /// Run the detected program's normal SBF build and include the result.
    #[arg(long, global = true)]
    pub(crate) build: bool,
    /// Build deadline in seconds (default 1800); requires --build.
    #[arg(long, global = true, requires = "build", value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub(crate) build_timeout_seconds: Option<u64>,
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Discover programs, clients, lock evidence, IDLs, artifacts, and RPC reads.
    Inspect(Common),
    /// Check the current project for reviewed Solana compatibility changes.
    Check(CheckArgs),
    /// Show breaking changes and migration actions for selected release targets.
    Upgrade(UpgradeArgs),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum Format {
    #[default]
    Terminal,
    Json,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub(crate) fn enabled(self) -> bool {
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
pub(crate) struct Common {
    #[arg(long, default_value = ".")]
    pub(crate) path: PathBuf,
    /// Explicit config file; otherwise read solcompat.toml inside --path when present.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "terminal")]
    pub(crate) format: Format,
    /// Select local, a dated snapshot, or a conditional deployment scenario.
    #[arg(long, conflicts_with = "target_file")]
    pub(crate) target: Option<String>,
    /// Select an explicit local JSON target/scenario document.
    #[arg(long, conflicts_with = "target")]
    pub(crate) target_file: Option<PathBuf>,
    /// Validate and use an explicit local dataset instead of the bundled revision.
    #[arg(long)]
    pub(crate) data: Option<PathBuf>,
    /// Import captured Cargo metadata; incompatible with --build's fresh snapshot.
    #[arg(long, conflicts_with = "build")]
    pub(crate) cargo_metadata: Option<PathBuf>,
    /// Inspect an existing ELF artifact. The path must be inside --path.
    #[arg(long = "artifact")]
    pub(crate) artifacts: Vec<PathBuf>,
    /// Run bounded `--version` probes for installed tools.
    #[arg(long)]
    pub(crate) probe_tools: bool,
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
pub(crate) struct CheckArgs {
    #[command(flatten)]
    pub(crate) common: Common,
    /// Expand evidence, explanations, migration steps, and references.
    #[arg(long)]
    pub(crate) detailed: bool,
    #[arg(long)]
    pub(crate) deny_warnings: bool,
    #[arg(long)]
    pub(crate) deny_unknown: bool,
}

#[derive(Args)]
pub(crate) struct UpgradeArgs {
    #[command(flatten)]
    pub(crate) common: Common,
    /// Anchor release to assess as the migration target.
    #[arg(long, default_value = "1.2.0")]
    pub(crate) anchor: String,
    /// Agave release to assess as the deployment/tooling target.
    #[arg(long, default_value = "4.3.0")]
    pub(crate) agave: String,
    /// Expand evidence, migration steps, and references.
    #[arg(long)]
    pub(crate) detailed: bool,
}
