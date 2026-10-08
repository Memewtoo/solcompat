//! Immutable evidence, validated compatibility data, and pure rule evaluation.
pub mod catalog;
pub mod data;
pub mod model;
mod report;
pub use report::Verdict;
mod evaluation;
pub mod rules;
mod signals;
mod target;
pub mod upgrade;
mod version;
pub use signals::SourceSignal;
pub use upgrade::upgrade_report;

pub use data::Dataset;
pub use model::*;
pub use rules::check;

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
