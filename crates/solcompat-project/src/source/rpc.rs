//! Recognize direct JS/TS RPC calls and literal options. Configured reads take precedence over scanning.

use crate::{
    discovery::EXCLUDED,
    input::{inside, label, read_text},
};
use anyhow::Result;
use solcompat_core::{digest, Client, Evidence, Omitted, RpcMaximum, RpcRead};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn source_files(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !EXCLUDED.contains(&name.as_ref()) && !entry.path().join("package.json").exists() {
                source_files(root, &entry.path(), out)?;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(extension, "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs") {
            out.push(inside(root, &path)?);
        }
    }
    Ok(())
}

use super::javascript::{self, Kind, Property};
use anyhow::ensure;

fn string_option(value: Property<'_>, default: &str) -> Option<String> {
    match value {
        Property::Missing => Some(default.into()),
        Property::Literal(text)
            if (text.starts_with('\'') || text.starts_with('"')) && !text.contains('\\') =>
        {
            Some(text[1..text.len() - 1].into())
        }
        _ => None,
    }
}

pub(crate) fn discover_rpc_reads(root: &Path, clients: &mut [Client]) -> Result<()> {
    for client in clients {
        if !client.rpc_reads.is_empty() {
            continue;
        }
        let manifest = root.join(&client.manifest);
        let Some(directory) = manifest.parent() else {
            continue;
        };
        let mut files = Vec::new();
        source_files(root, directory, &mut files)?;
        let mut sequence = 0;
        let mut providers = std::collections::BTreeSet::new();
        for path in files {
            let source = read_text(&path)?;
            let tokens = javascript::tokens(&source);
            let bindings = javascript::rpc_bindings(&tokens, &source);
            for method in ["getBlock", "getTransaction"] {
                for i in 0..tokens.len().saturating_sub(3) {
                    if tokens[i].kind != Kind::Word
                        || tokens[i + 1].text != "."
                        || tokens[i + 2].text != method
                        || tokens[i + 3].text != "("
                    {
                        continue;
                    }
                    let Some(end) = javascript::closing(&tokens, i + 3) else {
                        continue;
                    };
                    if tokens[end].end - tokens[i + 3].start > 16 * 1024 {
                        continue;
                    }
                    let arguments = javascript::split(&tokens[i + 4..end]);
                    let options = arguments.get(1).copied();
                    let provider = bindings
                        .get(tokens[i].text)
                        .filter(|_| i == 0 || tokens[i - 1].text != ".");
                    let known = provider.is_some();
                    if let Some(provider) = provider {
                        providers.insert(provider.clone());
                    }
                    let maximum = if !known {
                        None
                    } else {
                        match options {
                            None => Some(RpcMaximum::Omitted(Omitted::Explicit)),
                            Some(options) => match javascript::property(
                                options,
                                "maxSupportedTransactionVersion",
                            ) {
                                Property::Missing => Some(RpcMaximum::Omitted(Omitted::Explicit)),
                                Property::Literal(number) => {
                                    number.parse::<u8>().ok().map(RpcMaximum::Version)
                                }
                                Property::Unknown => None,
                            },
                        }
                    };
                    let encoding = if !known {
                        None
                    } else {
                        options.map_or_else(
                            || Some("json".into()),
                            |o| string_option(javascript::property(o, "encoding"), "json"),
                        )
                    };
                    let details = if method != "getBlock" || !known {
                        None
                    } else {
                        options.map_or_else(
                            || Some("full".into()),
                            |o| {
                                string_option(javascript::property(o, "transactionDetails"), "full")
                            },
                        )
                    };
                    sequence += 1;
                    let id = format!("source-{sequence}");
                    let line = source[..tokens[i + 2].start]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count()
                        + 1;
                    client.evidence.push(Evidence {
                        path: label(root, &path),
                        pointer: format!("/source:{line}/read:{id}"),
                        kind: if known {
                            "observed-rpc"
                        } else {
                            "unresolved-rpc"
                        }
                        .into(),
                        digest: digest(source.as_bytes()),
                    });
                    client.rpc_reads.push(RpcRead {
                        id,
                        method: method.into(),
                        encoding,
                        transaction_details: details,
                        max_supported_transaction_version: maximum,
                    });
                }
            }
        }
        let observed = client.evidence.iter().any(|ev| ev.kind == "observed-rpc");
        ensure!(!(observed && client.required_read_versions.as_ref().is_some_and(Vec::is_empty)), "client {} declares required_read_versions = [] but source contains a recognized transaction read",client.id);
        if observed {
            if client.required_read_versions.is_none() {
                client.required_read_versions =
                    Some(vec!["legacy".into(), "v0".into(), "v1".into()]);
            }
            // The schema has one decoder binding per client. Mixed providers need an explicit contract.
            if client.decoder_package.is_none() && providers.len() == 1 {
                let provider = providers.into_iter().next().unwrap();
                if crate::client::declared_client_package(client, &provider) {
                    client.decoder_package = Some(provider);
                }
            }
        }
    }
    Ok(())
}
