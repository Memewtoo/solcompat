//! Application sequencing; file/tool I/O stays in solcompat-project.
use crate::cli::{CheckArgs, Common, UpgradeArgs};
use anyhow::{bail, Result};
use solcompat_core::{
    check, AnalysisTarget, CheckResult, Counts, Dataset, DatasetIdentity, Outcome, Policy, Report,
    Severity,
};

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
        solcompat_project::enrich_tool_observations(
            &mut collected.project,
            solcompat_project::probe_tools(),
        );
    }
    Ok(collected)
}

pub(crate) fn analyze(common: &Common, options: Option<&CheckArgs>) -> Result<Report> {
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

pub(crate) fn analyze_upgrade(options: &UpgradeArgs) -> Result<Report> {
    let data = load_data(&options.common)?;
    let collected = collect(&options.common)?;
    solcompat_core::upgrade_report(collected.project, &data, &options.anchor, &options.agave)
}

pub(crate) fn append_build(
    report: &mut Report,
    common: &Common,
    options: &CheckArgs,
    timeout: std::time::Duration,
) -> Result<()> {
    if report.project.programs.len() != 1 {
        bail!(
            "--build requires exactly one detected program; found {}. Use --path to select one program project.",
            report.project.programs.len()
        );
    }
    let selected = report.project.programs[0].clone();
    let observation =
        solcompat_project::build_program_with_timeout(&common.path, &selected, timeout)?;
    // A build may create/change resolution files. Evaluate one fresh snapshot.
    let mut refreshed = analyze(common, Some(options))?;
    let program = refreshed
        .project
        .programs
        .iter()
        .find(|program| program.id == selected.id)
        .ok_or_else(|| {
            anyhow::anyhow!("built program is no longer present in the refreshed inventory")
        })?;
    let summary = if observation.success {
        format!("`{}` completed successfully.", observation.command)
    } else {
        format!("`{}` exited unsuccessfully.", observation.command)
    };
    refreshed.results.push(CheckResult {
        rule_id: "SC100".into(),
        rule_name: "SBF program build".into(),
        rule_revision: solcompat_core::catalog::rule("SC100")
            .expect("catalog rule")
            .binary_revision()
            .expect("binary metadata"),
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
    refreshed.finish();
    *report = refreshed;
    Ok(())
}
