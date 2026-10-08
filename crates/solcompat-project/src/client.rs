//! Construct and identify Client records from npm declarations and lockfiles.

use crate::config::{identifier, tx_version};
use crate::{
    input::{evidence, label, read_text},
    npm::{npm_lock, PackageJson},
};
use anyhow::{ensure, Context, Result};
use solcompat_core::{Client, RpcRead};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Construct the initial Client from package.json and its nearest supported
/// lockfile. Configured contracts and source observations are applied later.
pub(crate) fn collect_client(root: &Path, path: &Path, id: String) -> Result<Client> {
    let text = read_text(path)?;
    let p: PackageJson = serde_json::from_str(&text)
        .with_context(|| format!("invalid package manifest {}", path.display()))?;
    let mut deps = BTreeMap::new();
    for (kind, group) in [
        ("dependencies", p.dependencies),
        ("devDependencies", p.dev_dependencies),
        ("peerDependencies", p.peer_dependencies),
        ("optionalDependencies", p.optional_dependencies),
    ] {
        for (n, r) in group {
            deps.insert(format!("{kind}/{n}"), r);
        }
    }
    let declared = deps
        .keys()
        .filter_map(|key| key.split_once('/').map(|(_, name)| name.to_owned()))
        .collect::<BTreeSet<_>>();
    let (mut resolved_dependencies, resolution_evidence) = npm_lock(root, path)?;
    let mut client_evidence = vec![evidence(root, path, "/", "declared", text.as_bytes())];
    client_evidence.extend(resolution_evidence);
    resolved_dependencies.retain(|name, _| declared.contains(name));
    Ok(Client {
        id,
        name: p.name,
        manifest: label(root, path),
        dependencies: deps,
        resolved_dependencies: crate::resolution::wire_versions(&resolved_dependencies),
        decoder_package: None,
        required_read_versions: None,
        rpc_reads: vec![],
        evidence: client_evidence,
    })
}
pub(crate) fn is_solana_client(client: &Client) -> bool {
    client.dependencies.keys().any(|key| {
        key.split_once('/').is_some_and(|(_, name)| {
            name.starts_with("@solana/")
                || name == "@coral-xyz/anchor"
                || name.starts_with("@anchor-lang/")
        })
    })
}

pub(crate) fn declared_client_package(client: &Client, package: &str) -> bool {
    client
        .dependencies
        .keys()
        .any(|key| key.split_once('/').is_some_and(|(_, name)| name == package))
}

/// Apply an explicit contract before any source discovery. Validation and evidence
/// order are retained from the collection pipeline.
pub(crate) fn apply_client_config(
    client: &mut Client,
    c: &crate::config::ClientConfig,
    config_ev: Option<&solcompat_core::Evidence>,
    i: usize,
) -> Result<()> {
    if let Some(d) = &c.decoder_package {
        ensure!(
            client
                .dependencies
                .keys()
                .any(|k| k.split_once('/').is_some_and(|x| x.1 == d)),
            "decoder_package {d} is not declared in {}",
            client.manifest
        );
        client.decoder_package = Some(d.clone());
    }
    if let Some(v) = &c.required_read_versions {
        ensure!(
            v.iter().all(|x| tx_version(x)),
            "invalid required_read_versions"
        );
        ensure!(
            v.iter().collect::<BTreeSet<_>>().len() == v.len(),
            "duplicate required_read_versions for {}",
            c.id
        );
        let mut versions = v.clone();
        versions.sort();
        client.required_read_versions = Some(versions);
    }
    ensure!(
        !(client
            .required_read_versions
            .as_ref()
            .is_some_and(Vec::is_empty)
            && !c.rpc_reads.is_empty()),
        "client {} cannot declare required_read_versions = [] and configure RPC reads",
        c.id
    );
    let mut ids = BTreeSet::new();
    for (ri, r) in c.rpc_reads.iter().enumerate() {
        ensure!(
            matches!(r.method.as_str(), "getBlock" | "getTransaction"),
            "unsupported RPC method: {}",
            r.method
        );
        if let Some(encoding) = &r.encoding {
            ensure!(!encoding.trim().is_empty(), "encoding must not be empty");
        }
        if let Some(details) = &r.transaction_details {
            ensure!(
                r.method == "getBlock",
                "transaction_details is only valid for getBlock"
            );
            ensure!(
                matches!(
                    details.as_str(),
                    "full" | "accounts" | "signatures" | "none"
                ),
                "invalid transaction_details: {details}"
            );
        }
        let id = r.id.clone().unwrap_or_else(|| format!("read-{}", ri + 1));
        ensure!(
            identifier(&id) && ids.insert(id.clone()),
            "RPC read ids must be valid and unique"
        );
        client.rpc_reads.push(RpcRead {
            id,
            method: r.method.clone(),
            encoding: r.encoding.clone(),
            transaction_details: r.transaction_details.clone(),
            max_supported_transaction_version: r.max_supported_transaction_version.clone(),
        });
    }
    if let Some(ev) = config_ev {
        let mut ev = ev.clone();
        ev.pointer = format!("clients[{i}]");
        client.evidence.push(ev);
    }
    Ok(())
}
