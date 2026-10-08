//! Collect local Solana project inputs for compatibility evaluation.
//!
//! [`collect`] reads configuration and manifests, resolves available lockfile or
//! captured metadata facts, gathers source observations, and assembles an inventory.
//! It never invokes Cargo, Anchor, npm, or project scripts. [`probe_tools`] and
//! [`build_program`] are separate, opt-in execution boundaries.
//!
//! Construction lives in `program.rs` and `client.rs`; sequencing and enrichment
//! live in named helpers in those modules, orchestrated by `collect.rs`. See the repository's `docs/ARCHITECTURE.md` for field sources.

mod artifact;
mod cargo;
mod client;
mod collect;
mod config;
mod discovery;
mod execution;
mod idl;
mod input;
mod npm;
mod program;
mod resolution;
mod source;
mod tools;

pub use collect::{collect, Collected};
pub use input::read_text;
pub use tools::{
    build_program, build_program_with_timeout, enrich_tool_observations, probe_tools,
    BuildObservation,
};

#[cfg(test)]
mod tests;
