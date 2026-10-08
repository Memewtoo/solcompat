//! Internal dependency observations. Schema 1 exposes only unambiguous versions.

use std::collections::{BTreeMap, BTreeSet};

/// Keep source category and ambiguity until the model adapter is applied.
/// This does not strengthen the existing registry/edge identity assumptions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VersionResolution {
    CargoLock(BTreeSet<String>),
    NpmLock(String),
    CargoMetadata(String),
    Missing,
}

impl VersionResolution {
    pub(crate) fn exact(&self) -> Option<&str> {
        match self {
            Self::CargoLock(versions) if versions.len() == 1 => {
                versions.first().map(String::as_str)
            }
            Self::NpmLock(version) | Self::CargoMetadata(version) => Some(version),
            _ => None,
        }
    }
}

pub(crate) type Resolutions = BTreeMap<String, VersionResolution>;

/// The single adapter from resolution observations to the schema-1 version map.
pub(crate) fn wire_versions(observations: &Resolutions) -> BTreeMap<String, String> {
    observations
        .iter()
        .filter_map(|(name, observation)| {
            observation
                .exact()
                .map(|version| (name.clone(), version.into()))
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn lock_resolution(observations: &Resolutions, name: &str) -> VersionResolution {
    observations
        .get(name)
        .cloned()
        .unwrap_or(VersionResolution::Missing)
}
