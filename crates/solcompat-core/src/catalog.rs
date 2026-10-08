//! Static rule identity and ownership. Compatibility metadata remains versioned data.

/// Where a rule's metadata is currently maintained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataOwner {
    Dataset,
    RpcRecord,
    Binary { revision: u32 },
}

/// Discoverable ownership without runtime plugins or dynamic registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleDescriptor {
    pub id: &'static str,
    pub owner: &'static str,
    pub metadata: MetadataOwner,
    pub suppressible: bool,
}

/// Every current rule identity, including the opt-in build and upgrade rules.
pub const RULES: &[RuleDescriptor] = &[
    RuleDescriptor {
        id: "SC001",
        owner: "rules::program::rust_requirement",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC003",
        owner: "rules::anchor::cli_alignment",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC100",
        owner: "CLI app::append_build",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "SC102",
        owner: "rules::artifact::artifact_result",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC103",
        owner: "rules::program::sbpf_prerequisites",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC104",
        owner: "rules::pinocchio::framework_syntax_result",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC200",
        owner: "rules::decoder::decoder_result",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC201",
        owner: "rules::rpc::evaluate",
        metadata: MetadataOwner::RpcRecord,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC302",
        owner: "rules::anchor::crate_alignment",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "SC400",
        owner: "rules::idl::idl_result",
        metadata: MetadataOwner::Dataset,
        suppressible: true,
    },
    RuleDescriptor {
        id: "UP100",
        owner: "upgrade::anchor::baseline",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP101",
        owner: "upgrade::anchor::solana_crate_migration",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP102",
        owner: "upgrade::anchor::build_deploy_changes",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP103",
        owner: "upgrade::anchor::major_migration",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP104",
        owner: "upgrade::anchor::typescript_package_migration",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP105",
        owner: "upgrade::anchor::platform_baseline",
        metadata: MetadataOwner::Binary { revision: 2 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP106",
        owner: "upgrade::anchor::discriminator_migration",
        metadata: MetadataOwner::Binary { revision: 3 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP107",
        owner: "upgrade::anchor::cpi_context_migration",
        metadata: MetadataOwner::Binary { revision: 3 },
        suppressible: false,
    },
    RuleDescriptor {
        id: "UP200",
        owner: "upgrade::agave::evaluate",
        metadata: MetadataOwner::Binary { revision: 1 },
        suppressible: false,
    },
];

pub fn rule(id: &str) -> Option<&'static RuleDescriptor> {
    RULES.iter().find(|rule| rule.id == id)
}

impl RuleDescriptor {
    /// Rules owned by the binary have no dataset revision. Other rules get their
    /// revision from validated dataset metadata rather than this catalog.
    pub fn binary_revision(&self) -> Option<u32> {
        match self.metadata {
            MetadataOwner::Binary { revision } => Some(revision),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_ids_metadata_and_suppression_support_are_consistent() {
        let unique: BTreeSet<_> = RULES.iter().map(|rule| rule.id).collect();
        assert_eq!(unique.len(), RULES.len());
        let data = crate::Dataset::bundled().unwrap();
        let mut dataset_count = 0;
        for descriptor in RULES {
            assert!(!descriptor.owner.is_empty());
            match descriptor.metadata {
                MetadataOwner::Dataset => {
                    dataset_count += 1;
                    assert_eq!(data.rule(descriptor.id).rule_id, descriptor.id);
                }
                MetadataOwner::RpcRecord => assert_eq!(data.rpc_record().rule_id, descriptor.id),
                MetadataOwner::Binary { revision } => {
                    assert!(revision > 0);
                    assert!(!descriptor.suppressible);
                }
            }
        }
        assert_eq!(dataset_count, 8);
        assert_eq!(RULES.iter().filter(|rule| rule.suppressible).count(), 9);
        assert!(rule("SC100").unwrap().binary_revision().is_some());
        assert!(rule("SC201").unwrap().binary_revision().is_none());
        assert!(rule("invalid").is_none());
    }
}
