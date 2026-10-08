//! Parse explicitly bound IDLs and identify their schema.

use crate::{
    config::IdlConfig,
    input::{evidence, inside, label, read_text},
};
use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use solcompat_core::IdlInput;
use std::path::Path;

pub(crate) fn parse_idl(root: &Path, c: &IdlConfig, base: &Path) -> Result<IdlInput> {
    let p = inside(root, &base.join(&c.path))?;
    let text = read_text(&p)?;
    let v: JsonValue =
        serde_json::from_str(&text).with_context(|| format!("invalid IDL {}", p.display()))?;
    let schema = if let Some(s) = v.pointer("/metadata/spec").and_then(|v| v.as_str()) {
        format!("anchor-spec-{s}")
    } else if v.get("version").is_some() && v.get("instructions").is_some() {
        "anchor-legacy".into()
    } else {
        "unknown".into()
    };
    Ok(IdlInput {
        id: c.id.clone(),
        path: label(root, &p),
        client: c.client.clone(),
        reader_package: c.reader_package.clone(),
        schema,
        evidence: vec![evidence(root, &p, "/", "observed", text.as_bytes())],
    })
}
