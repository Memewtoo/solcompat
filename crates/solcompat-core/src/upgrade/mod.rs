//! Pure migration advice for selected Anchor and Agave release targets.
mod agave;
mod anchor;
mod common;
use crate::version::version;
use crate::{Counts, Dataset, DatasetIdentity, Policy, Project, Report};
use anyhow::{bail, Result};
use semver::Version;

/// Evaluate reviewed migration boundaries using an already collected inventory.
/// This does not build, probe tools, or contact a cluster. Default report policy
/// and existing upgrade rule revisions are retained.
pub fn upgrade_report(
    project: Project,
    data: &Dataset,
    anchor_target: &str,
    agave_target: &str,
) -> Result<Report> {
    let anchor_target_version = version(anchor_target, "Anchor target")?;
    let agave_target_version = version(agave_target, "Agave target")?;
    if anchor_target_version < Version::new(0, 31, 0) {
        bail!("Anchor upgrade target must be 0.31.0 or newer");
    }
    let mut results = anchor::evaluate(&project, &anchor_target_version);
    results.extend(agave::evaluate(&agave_target_version));

    let mut report = Report {
        schema_version: 1,
        command: "upgrade".into(),
        analysis_mode: "upgrade-advice".into(),
        target: format!("anchor-{anchor_target_version}+agave-{agave_target_version}"),
        target_digest: None,
        dataset: DatasetIdentity {
            revision: data.revision.clone(),
            digest: data.digest.clone(),
        },
        project,
        results,
        counts: Counts::default(),
        policy: Policy::default(),
        exit_code: 0,
    };
    report.finish();
    Ok(report)
}

#[cfg(test)]
mod tests;
