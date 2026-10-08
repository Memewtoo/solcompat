//! Typed framework observations with an adapter for schema-1 evidence kinds.

use crate::{Evidence, Program};

/// A recognized source construct, not proof of semantic symbol identity.
/// Variant ordering preserves the existing evidence ordering by kind string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceSignal {
    AnchorCpiContextAccountInfo,
    AnchorDiscriminatorMethod,
    PinocchioAccountInfo,
    PinocchioImmutableEntrypoint,
    PinocchioMutableEntrypoint,
    PinocchioPubkey,
}

impl SourceSignal {
    /// Existing schema-1 identifier; serialized models continue to use strings.
    pub const fn evidence_kind(self) -> &'static str {
        match self {
            Self::AnchorCpiContextAccountInfo => "anchor-cpi-context-account-info",
            Self::AnchorDiscriminatorMethod => "anchor-discriminator-method",
            Self::PinocchioAccountInfo => "pinocchio-account-info",
            Self::PinocchioImmutableEntrypoint => "pinocchio-immutable-entrypoint",
            Self::PinocchioMutableEntrypoint => "pinocchio-mutable-entrypoint",
            Self::PinocchioPubkey => "pinocchio-pubkey",
        }
    }

    // Preserve broad prefix filtering for callers' existing schema-1 evidence.
    // Recognizing a typed signal and selecting framework evidence are distinct.
    pub(crate) fn is_framework_evidence(kind: &str) -> bool {
        Self::is_pinocchio_evidence(kind) || kind.starts_with("anchor-")
    }

    pub(crate) fn is_pinocchio_evidence(kind: &str) -> bool {
        kind.starts_with("pinocchio-")
    }

    pub(crate) fn present(self, program: &Program) -> bool {
        program
            .evidence
            .iter()
            .any(|item| item.kind == self.evidence_kind())
    }

    pub(crate) fn evidence(self, program: &Program) -> Option<Vec<Evidence>> {
        let evidence: Vec<_> = program
            .evidence
            .iter()
            .filter(|item| item.kind == self.evidence_kind())
            .cloned()
            .collect();
        (!evidence.is_empty()).then_some(evidence)
    }
}
