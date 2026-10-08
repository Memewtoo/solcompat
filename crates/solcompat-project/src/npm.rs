//! npm manifest shapes and supported package-lock resolution.
use crate::resolution::{Resolutions, VersionResolution};

use crate::input::{exists, inside, read_text};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PackageJson {
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    pub(crate) dev_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    pub(crate) peer_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    pub(crate) optional_dependencies: BTreeMap<String, String>,
}

pub(crate) fn npm_lock(
    root: &Path,
    manifest: &Path,
) -> Result<(Resolutions, Option<solcompat_core::Evidence>)> {
    let mut dir = manifest.parent();
    while let Some(d) = dir {
        if !d.starts_with(root) {
            break;
        }
        let p = d.join("package-lock.json");
        if exists(&p)? {
            let text = read_text(&inside(root, &p)?)?;
            let v: JsonValue = serde_json::from_str(&text).context("invalid package-lock.json")?;
            ensure!(
                matches!(
                    v.get("lockfileVersion").and_then(JsonValue::as_u64),
                    Some(2 | 3)
                ),
                "package-lock.json must use lockfileVersion 2 or 3"
            );
            let mut out = BTreeMap::new();
            if let Some(packages) = v.get("packages").and_then(JsonValue::as_object) {
                let manifest_text = read_text(manifest)?;
                let document: PackageJson = serde_json::from_str(&manifest_text)?;
                let declared = document
                    .dependencies
                    .into_iter()
                    .chain(document.dev_dependencies)
                    .chain(document.peer_dependencies)
                    .chain(document.optional_dependencies);
                let relative = manifest
                    .parent()
                    .unwrap()
                    .strip_prefix(d)
                    .unwrap()
                    .to_path_buf();
                for (name, requirement) in declared {
                    if requirement.starts_with("npm:")
                        || requirement.starts_with("file:")
                        || requirement.starts_with("git")
                        || requirement.contains("://")
                    {
                        continue;
                    }
                    let mut directory = relative.as_path();
                    loop {
                        let location = if directory.as_os_str().is_empty() {
                            format!("node_modules/{name}")
                        } else {
                            format!(
                                "{}/node_modules/{name}",
                                directory.to_string_lossy().replace('\\', "/")
                            )
                        };
                        if let Some(package) = packages.get(&location) {
                            let registry = package
                                .get("resolved")
                                .and_then(JsonValue::as_str)
                                .is_some_and(|s| {
                                    s.starts_with(&format!("https://registry.npmjs.org/{name}/-/"))
                                });
                            let identity = package
                                .get("name")
                                .and_then(JsonValue::as_str)
                                .is_none_or(|n| n == name);
                            if registry
                                && identity
                                && package.get("link").and_then(JsonValue::as_bool) != Some(true)
                            {
                                if let Some(version) =
                                    package.get("version").and_then(JsonValue::as_str)
                                {
                                    let requirement = requirement.replace(".x", ".*");
                                    if !semver::VersionReq::parse(&requirement).is_ok_and(|r| {
                                        semver::Version::parse(version).is_ok_and(|v| r.matches(&v))
                                    }) {
                                        break;
                                    }
                                    out.insert(
                                        name.clone(),
                                        VersionResolution::NpmLock(version.into()),
                                    );
                                }
                            }
                            break;
                        }
                        let Some(parent) = directory.parent() else {
                            break;
                        };
                        directory = parent;
                    }
                }
            }
            return Ok((
                out,
                Some(crate::input::evidence(
                    root,
                    &inside(root, &p)?,
                    "/packages",
                    "resolution",
                    text.as_bytes(),
                )),
            ));
        }
        dir = d.parent()
    }
    Ok((BTreeMap::new(), None))
}
