//! Immutable evidence, validated compatibility data, and pure rule evaluation.
pub mod data;
pub mod model;
pub mod rules;

pub use data::Dataset;
pub use model::*;
pub use rules::check;

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
