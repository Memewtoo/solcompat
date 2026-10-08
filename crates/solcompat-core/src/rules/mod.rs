//! Pure current-state evaluation, dispatched by component and rule family.
mod anchor;
mod artifact;
mod decoder;
mod idl;
mod pinocchio;
mod program;
mod rpc;

use crate::{
    AnalysisTarget, Counts, Dataset, DatasetIdentity, Policy, Program, Project, Report, Suppression,
};
use anyhow::Result;
use artifact::artifact_result;
use decoder::decoder_result;
use idl::idl_result;
use pinocchio::framework_syntax_result;
use rpc::evaluate;

fn program_results(program: &Program, data: &Dataset) -> Vec<crate::CheckResult> {
    vec![
        program::rust_requirement(program, data),
        anchor::cli_alignment(program, data),
        program::sbpf_prerequisites(program, data),
        anchor::crate_alignment(program, data),
    ]
}

/// Evaluate collected project facts against reviewed data and a selected target.
/// This function performs no filesystem reads or tool execution. It applies
/// configured suppressions and finalizes counts and exit policy before returning.
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
    crate::report::apply_suppressions(&mut results, suppressions)?;
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
mod tests;
