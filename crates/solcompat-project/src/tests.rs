//! Collector contracts tested without subprocesses or external project fixtures.
use crate::{collect, enrich_tool_observations};
use solcompat_core::{digest, SourceSignal, ToolObservation};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct ProjectDir(PathBuf);
impl ProjectDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solcompat-collector-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn anchor(&self) {
        self.write("Cargo.toml", "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\nanchor-lang='0.31'\nanchor-spl='0.31'\n");
        self.write("src/lib.rs", "#[program]\nmod example {}\n");
        self.write("Anchor.toml", "[toolchain]\nanchor_version='0.31.1'\n");
    }
    fn collect(&self, metadata: Option<&Path>) -> crate::Collected {
        collect(&self.0, None, None, metadata, &[]).unwrap()
    }
}
impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tool(name: &str, version: &str) -> ToolObservation {
    ToolObservation {
        tool: name.into(),
        version: Some(version.into()),
        executable: Some(format!("/test/{name}")),
        status: "observed".into(),
    }
}

#[test]
fn configuration_precedes_anchor_manifest_and_probes_with_source_evidence() {
    let dir = ProjectDir::new();
    dir.anchor();
    let config = "schema_version=1\n[[programs]]\nid='example'\nmanifest='Cargo.toml'\nanchor_cli_version='0.30.1'\nsbf_rust_version='1.79.0'\ncargo_build_sbf_version='4.2.0'\n";
    dir.write("solcompat.toml", config);
    let mut collected = dir.collect(None);
    enrich_tool_observations(
        &mut collected.project,
        vec![
            tool("anchor", "1.2.0"),
            tool("cargo-build-sbf", "4.3.0"),
            tool("rustc", "1.99.0"),
        ],
    );
    let program = &collected.project.programs[0];
    assert_eq!(program.anchor_cli_version.as_deref(), Some("0.30.1"));
    assert_eq!(program.cargo_build_sbf_version.as_deref(), Some("4.2.0"));
    assert_eq!(program.sbf_rust_version.as_deref(), Some("1.79.0"));
    assert!(program.evidence.iter().any(|ev| ev.path == "solcompat.toml"
        && ev.pointer == "programs[0]"
        && ev.kind == "asserted"
        && ev.digest == digest(config.as_bytes())));
    assert!(program
        .evidence
        .iter()
        .any(|ev| ev.path == "Anchor.toml" && ev.pointer == "/toolchain" && ev.kind == "declared"));
}

#[test]
fn probes_fill_only_missing_versions_and_never_select_sbf_rust() {
    let dir = ProjectDir::new();
    dir.anchor();
    let mut collected = dir.collect(None);
    enrich_tool_observations(
        &mut collected.project,
        vec![
            tool("anchor", "1.2.0"),
            tool("cargo-build-sbf", "4.3.0"),
            tool("rustc", "1.99.0"),
        ],
    );
    let program = &collected.project.programs[0];
    assert_eq!(program.anchor_cli_version.as_deref(), Some("0.31.1"));
    assert_eq!(program.cargo_build_sbf_version.as_deref(), Some("4.3.0"));
    assert_eq!(program.sbf_rust_version, None);
    fs::remove_file(dir.0.join("Anchor.toml")).unwrap();
    let mut collected = dir.collect(None);
    enrich_tool_observations(&mut collected.project, vec![tool("anchor", "1.2.0")]);
    assert_eq!(
        collected.project.programs[0].anchor_cli_version.as_deref(),
        Some("1.2.0")
    );
}

#[test]
fn ambiguous_lock_versions_remain_typed_and_unresolved() {
    let dir = ProjectDir::new();
    dir.anchor();
    dir.write("Cargo.lock", "version=3\n[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n[[package]]\nname='anchor-lang'\nversion='0.32.1'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n");
    let observations = crate::cargo::lock_versions(&dir.0, &dir.0).unwrap();
    match crate::resolution::lock_resolution(&observations.versions, "anchor-lang") {
        crate::resolution::VersionResolution::CargoLock(versions) => assert_eq!(versions.len(), 2),
        other => panic!("expected ambiguous lock resolution, got {other:?}"),
    }
    assert_eq!(
        crate::resolution::lock_resolution(&observations.versions, "anchor-spl"),
        crate::resolution::VersionResolution::Missing
    );
    assert!(!dir.collect(None).project.programs[0]
        .resolved_dependencies
        .contains_key("anchor-lang"));
}

#[test]
fn matching_metadata_replaces_lock_map_even_when_empty() {
    let dir = ProjectDir::new();
    dir.anchor();
    dir.write("Cargo.lock", "version=3\n[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['anchor-lang']\n");
    assert_eq!(
        dir.collect(None).project.programs[0].resolved_dependencies["anchor-lang"],
        "0.31.1"
    );
    let metadata = serde_json::json!({"version":1,"packages":[
        {"id":"program","name":"example","version":"0.1.0","manifest_path":dir.0.join("Cargo.toml")},
        {"id":"anchor","name":"anchor-lang","version":"0.31.2","source":"registry+https://github.com/rust-lang/crates.io-index"}
    ],"resolve":{"nodes":[{"id":"program","deps":[{"pkg":"anchor", "dep_kinds":[{"kind":null,"target":null}]}]}]}});
    dir.write("metadata.json", &metadata.to_string());
    assert_eq!(
        dir.collect(Some(&dir.0.join("metadata.json")))
            .project
            .programs[0]
            .resolved_dependencies["anchor-lang"],
        "0.31.2"
    );
    let mut stale = metadata.clone();
    stale["packages"][1]["version"] = serde_json::json!("0.32.1");
    dir.write("metadata.json", &stale.to_string());
    assert!(dir
        .collect(Some(&dir.0.join("metadata.json")))
        .project
        .programs[0]
        .resolved_dependencies
        .is_empty());
    dir.write("Cargo.toml", "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\nanchor-lang={version='0.31',git='https://example.test/fork'}\n");
    dir.write("metadata.json", &metadata.to_string());
    assert!(dir
        .collect(Some(&dir.0.join("metadata.json")))
        .project
        .programs[0]
        .resolved_dependencies
        .is_empty());
    dir.anchor();
    let mut empty = metadata;
    empty["resolve"]["nodes"][0]["deps"] = serde_json::json!([]);
    dir.write("metadata.json", &empty.to_string());
    assert!(dir
        .collect(Some(&dir.0.join("metadata.json")))
        .project
        .programs[0]
        .resolved_dependencies
        .is_empty());
}

#[test]
fn configured_reads_precede_source_and_preserve_asserted_evidence() {
    let dir = ProjectDir::new();
    dir.write(
        "package.json",
        r#"{"name":"client","dependencies":{"@solana/kit":"^8"}}"#,
    );
    dir.write(
        "client.ts",
        "import { createSolanaRpc } from '@solana/kit'; const rpc = createSolanaRpc('url'); rpc.getBlock(42, {maxSupportedTransactionVersion: 0});",
    );
    dir.write("solcompat.toml", "schema_version=1\n[[clients]]\nid='client'\nmanifest='package.json'\nrequired_read_versions=['v1','legacy']\n[[clients.rpc_reads]]\nid='chosen'\nmethod='getTransaction'\nencoding='json'\nmax_supported_transaction_version=1\n");
    let collected = dir.collect(None);
    let client = &collected.project.clients[0];
    assert_eq!(client.rpc_reads.len(), 1);
    assert_eq!(client.rpc_reads[0].id, "chosen");
    assert_eq!(
        client.required_read_versions.as_ref().unwrap(),
        &vec!["legacy".to_string(), "v1".to_string()]
    );
    assert!(client.evidence.iter().any(|ev| ev.path == "solcompat.toml"
        && ev.pointer == "clients[0]"
        && ev.kind == "asserted"));
    assert!(!client.evidence.iter().any(|ev| ev.path == "client.ts"));
    fs::remove_file(dir.0.join("solcompat.toml")).unwrap();
    let collected = dir.collect(None);
    let client = &collected.project.clients[0];
    assert_eq!(client.rpc_reads[0].id, "source-1");
    assert_eq!(client.decoder_package.as_deref(), Some("@solana/kit"));
    assert!(client
        .evidence
        .iter()
        .any(|ev| ev.path == "client.ts" && ev.kind == "observed-rpc"));
}

#[test]
fn typed_signals_keep_schema_one_order_and_source_digests() {
    let dir = ProjectDir::new();
    dir.anchor();
    let source = "#[program]\nmod example {}\nfn example() { Type::discriminator(); CpiContext::new(program.to_account_info(), accounts); }\n";
    dir.write("src/lib.rs", source);
    let collected = dir.collect(None);
    let program = &collected.project.programs[0];
    let signals: Vec<_> = program
        .evidence
        .iter()
        .filter(|ev| ev.kind.starts_with("anchor-"))
        .collect();
    assert_eq!(
        signals
            .iter()
            .map(|ev| ev.kind.as_str())
            .collect::<Vec<_>>(),
        vec![
            SourceSignal::AnchorCpiContextAccountInfo.evidence_kind(),
            SourceSignal::AnchorDiscriminatorMethod.evidence_kind()
        ]
    );
    assert!(signals.iter().all(|ev| ev.path == "src/lib.rs"
        && ev.pointer.starts_with("/source:")
        && ev.digest == digest(source.as_bytes())));
}

#[test]
fn npm_declarations_and_lock_observations_remain_separate() {
    let dir = ProjectDir::new();
    dir.write(
        "package.json",
        r#"{"name":"client","dependencies":{"@solana/kit":"^8"}}"#,
    );
    dir.write("package-lock.json", r#"{"lockfileVersion":3,"packages":{"node_modules/@solana/kit":{"version":"8.1.0","resolved":"https://registry.npmjs.org/@solana/kit/-/kit-8.1.0.tgz"}}}"#);
    let observations = crate::npm::npm_lock(&dir.0, &dir.0.join("package.json")).unwrap();
    assert_eq!(
        observations.0["@solana/kit"],
        crate::resolution::VersionResolution::NpmLock("8.1.0".into())
    );
    let collected = dir.collect(None);
    let client = &collected.project.clients[0];
    assert_eq!(client.dependencies["dependencies/@solana/kit"], "^8");
    assert_eq!(client.resolved_dependencies["@solana/kit"], "8.1.0");
    assert!(client.rpc_reads.is_empty());
    assert_eq!(client.decoder_package, None);
}

#[test]
fn comments_and_strings_are_not_program_syntax() {
    let dir = ProjectDir::new();
    dir.anchor();
    dir.write("src/lib.rs", "#[program]\nmod example {}\n// Type::discriminator();\nconst TEXT: &str = \"CpiContext::new(program.to_account_info(), accounts)\";\n");
    let collected = dir.collect(None);
    assert!(!collected.project.programs[0]
        .evidence
        .iter()
        .any(|ev| ev.kind.starts_with("anchor-")));
}

#[test]
fn dynamic_rpc_properties_are_not_omissions_or_literal_prefixes() {
    let dir = ProjectDir::new();
    dir.write("package.json", r#"{"dependencies":{"@solana/kit":"^8"}}"#);
    dir.write(
        "client.ts",
        "import { createSolanaRpc } from '@solana/kit'; const rpc = createSolanaRpc('url'); rpc.getBlock(42, { maxSupportedTransactionVersion: 1 + offset });\n",
    );
    let collected = dir.collect(None);
    assert_eq!(
        collected.project.clients[0].rpc_reads[0].max_supported_transaction_version,
        None
    );
}

#[test]
fn lockfiles_do_not_trust_registry_substrings() {
    let dir = ProjectDir::new();
    dir.anchor();
    dir.write("Cargo.lock", "version=3\n[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='git+https://example.test/crates.io/fork'\n");
    assert!(!dir.collect(None).project.programs[0]
        .resolved_dependencies
        .contains_key("anchor-lang"));
}

#[test]
fn lock_edges_and_declared_sources_must_identify_the_upstream_dependency() {
    let dir = ProjectDir::new();
    dir.anchor();
    let upstream = "[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n";
    let owner = "[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['anchor-lang']\n";
    dir.write("Cargo.lock", &format!("version=3\n{upstream}"));
    assert!(dir.collect(None).project.programs[0]
        .resolved_dependencies
        .is_empty());
    dir.write("Cargo.lock", &format!("version=3\n{upstream}{owner}"));
    assert_eq!(
        dir.collect(None).project.programs[0].resolved_dependencies["anchor-lang"],
        "0.31.1"
    );
    dir.write("Cargo.toml", "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\nrenamed={package='anchor-lang',version='0.31',git='https://example.test/fork'}\n");
    assert!(dir.collect(None).project.programs[0]
        .resolved_dependencies
        .is_empty());
    dir.anchor();
    dir.write("Cargo.lock", &format!("version=3\n{upstream}{owner}[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='git+https://example.test/fork'\n"));
    assert!(dir.collect(None).project.programs[0]
        .resolved_dependencies
        .is_empty());
}

#[test]
fn workspace_aliases_preserve_package_identity_and_resolution_provenance() {
    let dir = ProjectDir::new();
    dir.write("Cargo.toml", "[workspace]\nmembers=['program']\n[workspace.dependencies]\nframework={package='anchor-lang',version='0.31'}\n");
    dir.write(
        "program/Cargo.toml",
        "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\nframework.workspace=true\n",
    );
    dir.write("program/src/lib.rs", "#[program]\nmod example {}\n");
    let lock="version=3\n[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['anchor-lang']\n[[package]]\nname='anchor-lang'\nversion='0.31.1'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n";
    dir.write("Cargo.lock", lock);
    let collected = dir.collect(None);
    let program = &collected.project.programs[0];
    assert_eq!(program.framework, "anchor");
    assert_eq!(
        program.dependencies["dependencies/framework=>anchor-lang"],
        "0.31"
    );
    assert_eq!(program.resolved_dependencies["anchor-lang"], "0.31.1");
    assert!(program.evidence.iter().any(|ev| ev.path == "Cargo.lock"
        && ev.kind == "resolution"
        && ev.digest == digest(lock.as_bytes())));
    assert!(program
        .evidence
        .iter()
        .any(|ev| ev.path == "Cargo.toml" && ev.pointer == "/workspace/dependencies"));
}

#[test]
fn rpc_literals_defaults_and_unknown_expressions_remain_distinct() {
    let dir = ProjectDir::new();
    dir.write(
        "package.json",
        r#"{"dependencies":{"@solana/kit":"^8","@solana/web3.js":"^1"}}"#,
    );
    let prefix =
        "import { Connection } from '@solana/web3.js'; const connection = new Connection('url');\n";
    for (options, expected) in [
        (
            "{maxSupportedTransactionVersion: 1}",
            Some(solcompat_core::RpcMaximum::Version(1)),
        ),
        (
            "{}",
            Some(solcompat_core::RpcMaximum::Omitted(
                solcompat_core::Omitted::Explicit,
            )),
        ),
        ("{...defaults,maxSupportedTransactionVersion:1}", None),
        (
            "{maxSupportedTransactionVersion:0,maxSupportedTransactionVersion:1}",
            None,
        ),
        ("{maxSupportedTransactionVersion:1+offset}", None),
        ("options", None),
        ("{['maxSupportedTransactionVersion']:1}", None),
    ] {
        dir.write(
            "client.ts",
            &format!("{prefix}connection.getTransaction('signature', {options});"),
        );
        let collected = dir.collect(None);
        let client = &collected.project.clients[0];
        assert_eq!(
            client.rpc_reads[0].max_supported_transaction_version, expected,
            "{options}"
        );
        assert_eq!(client.decoder_package.as_deref(), Some("@solana/web3.js"));
    }
    dir.write("client.ts",&format!("{prefix}// connection.getBlock(42);\nconst text='connection.getBlock(42)'; const re=/connection.getBlock(42)/;"));
    assert!(dir.collect(None).project.clients[0].rpc_reads.is_empty());
}

#[test]
fn unbound_shadowed_and_mixed_rpc_providers_do_not_borrow_a_decoder() {
    let dir = ProjectDir::new();
    dir.write(
        "package.json",
        r#"{"dependencies":{"@solana/kit":"^8","@solana/web3.js":"^1"}}"#,
    );
    for source in [
        "function read(rpc) { rpc.getBlock(42); }",
        "import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url'); function read(rpc) { rpc.getBlock(42); }",
        "import {createSolanaRpc} from '@solana/web3.js'; const rpc=createSolanaRpc('url'); rpc.getBlock(42);",
        "import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url'); rpc=other; rpc.getBlock(42);",
        "import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url') || other; rpc.getBlock(42);",
        "import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url').other; rpc.getBlock(42);",
        "import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url'); const read = rpc => rpc.getBlock(42);",
    ] {
        dir.write("client.ts",source);let collected=dir.collect(None);let client=&collected.project.clients[0];
        assert_eq!(client.rpc_reads.len(),1);
        assert_eq!(client.decoder_package,None);
        assert_eq!(client.rpc_reads[0].max_supported_transaction_version,None);
        assert!(client.evidence.iter().any(|ev|ev.kind=="unresolved-rpc"));
    }
    dir.write("client.ts","import {createSolanaRpc} from '@solana/kit'; import {Connection} from '@solana/web3.js'; const rpc=createSolanaRpc('url'); const connection=new Connection('url'); rpc.getBlock(42); connection.getTransaction('sig');");
    assert_eq!(dir.collect(None).project.clients[0].decoder_package, None);
}

#[test]
fn nested_client_source_and_lock_versions_belong_to_their_own_package() {
    let dir = ProjectDir::new();
    for manifest in ["package.json", "child/package.json"] {
        dir.write(manifest, r#"{"dependencies":{"@solana/kit":"^8"}}"#);
    }
    dir.write("child/client.ts","import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url'); rpc.getBlock(42);");
    dir.write("package-lock.json",r#"{"lockfileVersion":3,"packages":{"node_modules/@solana/kit":{"version":"8.0.0","resolved":"https://registry.npmjs.org/@solana/kit/-/kit-8.0.0.tgz"},"child/node_modules/@solana/kit":{"version":"8.1.0","resolved":"https://registry.npmjs.org/@solana/kit/-/kit-8.1.0.tgz"}}}"#);
    let collected = dir.collect(None);
    let parent = collected
        .project
        .clients
        .iter()
        .find(|c| c.manifest == "package.json")
        .unwrap();
    let child = collected
        .project
        .clients
        .iter()
        .find(|c| c.manifest == "child/package.json")
        .unwrap();
    assert!(parent.rpc_reads.is_empty());
    assert_eq!(child.rpc_reads.len(), 1);
    assert_eq!(parent.resolved_dependencies["@solana/kit"], "8.0.0");
    assert_eq!(child.resolved_dependencies["@solana/kit"], "8.1.0");
}

#[test]
fn rust_signals_follow_declared_modules_and_use_actual_locations() {
    let dir = ProjectDir::new();
    dir.anchor();
    dir.write(
        "src/lib.rs",
        "#[program]\nmod example {}\nmod instruction;\n",
    );
    dir.write("src/unused.rs", "fn example(){ Type::discriminator(); }");
    dir.write(
        "src/instruction.rs",
        "fn one(){ Type::discriminator(); }\n\nfn two(){ Type::discriminator(); }\n",
    );
    let collected = dir.collect(None);
    let evidence = &collected.project.programs[0].evidence;
    let signals: Vec<_> = evidence
        .iter()
        .filter(|ev| ev.kind == "anchor-discriminator-method")
        .collect();
    assert_eq!(signals.len(), 2);
    assert_eq!(signals[0].path, "src/instruction.rs");
    assert_eq!(signals[0].pointer, "/source:1");
    assert_eq!(signals[1].pointer, "/source:3");
    dir.write(
        "src/instruction.rs",
        "#[cfg(feature=\"legacy\")]\nfn example(){ Type::discriminator(); }",
    );
    let collected = dir.collect(None);
    assert!(!collected.project.programs[0]
        .evidence
        .iter()
        .any(|ev| ev.kind == "anchor-discriminator-method"));
    assert!(collected.project.programs[0]
        .evidence
        .iter()
        .any(|ev| ev.kind == "unresolved-rust"));
}

#[test]
fn no_read_contract_cannot_contradict_a_recognized_source_read() {
    let dir = ProjectDir::new();
    dir.write("package.json", r#"{"dependencies":{"@solana/kit":"^8"}}"#);
    dir.write("client.ts","import {createSolanaRpc} from '@solana/kit'; const rpc=createSolanaRpc('url'); rpc.getBlock(42);");
    dir.write("solcompat.toml","schema_version=1\n[[clients]]\nid='client'\nmanifest='package.json'\nrequired_read_versions=[]\n");
    let error = collect(&dir.0, None, None, None, &[])
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("source contains a recognized transaction read"));
}

#[test]
fn conditional_or_build_only_metadata_edges_do_not_prove_program_versions() {
    let dir = ProjectDir::new();
    dir.anchor();
    let mut metadata = serde_json::json!({"version":1,"packages":[
        {"id":"program","name":"example","version":"0.1.0","manifest_path":dir.0.join("Cargo.toml")},
        {"id":"anchor","name":"anchor-lang","version":"0.31.1","source":"registry+https://github.com/rust-lang/crates.io-index"}
    ],"resolve":{"nodes":[{"id":"program","deps":[{"pkg":"anchor","dep_kinds":[]}]}]}});
    for kinds in [
        serde_json::json!([]),
        serde_json::json!([{"kind":"build","target":null}]),
        serde_json::json!([{"kind":null,"target":"cfg(unix)"}]),
    ] {
        metadata["resolve"]["nodes"][0]["deps"][0]["dep_kinds"] = kinds;
        dir.write("metadata.json", &metadata.to_string());
        assert!(dir
            .collect(Some(&dir.0.join("metadata.json")))
            .project
            .programs[0]
            .resolved_dependencies
            .is_empty());
    }
}

fn pinocchio_source_result(source: &str) -> solcompat_core::Report {
    let dir = ProjectDir::new();
    dir.write(
        "Cargo.toml",
        "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\npinocchio='0.11'\n",
    );
    dir.write("Cargo.lock", "version=3\n[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['pinocchio']\n[[package]]\nname='pinocchio'\nversion='0.11.2'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n");
    dir.write("src/lib.rs", source);
    solcompat_core::check(
        dir.collect(None).project,
        &solcompat_core::Dataset::bundled().unwrap(),
        solcompat_core::Policy::default(),
        &[],
        &solcompat_core::AnalysisTarget::named("local").unwrap(),
    )
    .unwrap()
}

#[test]
fn pinocchio_entrypoint_account_parameter_names_do_not_change_alignment() {
    for (reference, expected) in [
        ("&mut", solcompat_core::Outcome::Pass),
        ("&", solcompat_core::Outcome::Finding),
    ] {
        for name in ["accounts", "_accounts", "account_views", "_"] {
            let report = pinocchio_source_result(&format!("pinocchio::program_entrypoint!(process_instruction);\nfn process_instruction(id: &Address, {name}: {reference} [AccountView], data: &[u8]) {{}}\n"));
            assert_eq!(
                report
                    .results
                    .iter()
                    .find(|r| r.rule_id == "SC104")
                    .unwrap()
                    .outcome,
                expected,
                "{name}"
            );
        }
    }
}

#[test]
fn conditional_entrypoint_imports_do_not_hide_unconditional_signatures() {
    let report = pinocchio_source_result("#[cfg(not(feature=\"no-entrypoint\"))]\nuse pinocchio::entrypoint;\n#[cfg(not(feature=\"no-entrypoint\"))]\nentrypoint!(process_instruction);\nfn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}\n");
    assert_eq!(
        report
            .results
            .iter()
            .find(|r| r.rule_id == "SC104")
            .unwrap()
            .outcome,
        solcompat_core::Outcome::Pass
    );
    assert!(!report.project.programs[0]
        .evidence
        .iter()
        .any(|ev| ev.kind == "unresolved-rust"));
}

#[test]
fn conditional_framework_evidence_and_missing_modules_remain_unresolved() {
    for source in [
        "#[cfg(feature=\"variant\")] fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}",
        "#[cfg_attr(feature=\"variant\",cfg(unix))] fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}",
        "#[cfg(feature=\"legacy\")] use pinocchio::account_info::AccountInfo; fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}",
        "mod missing; fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}",
        "#[cfg(feature=\"variant\")] mod variant; fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}",

    ] {
        let report=pinocchio_source_result(&format!("pinocchio::program_entrypoint!(process_instruction);\n{source}"));
        assert_eq!(report.results.iter().find(|r|r.rule_id=="SC104").unwrap().outcome, solcompat_core::Outcome::Unknown,"{source}");
    }
}

#[test]
fn pinocchio_account_slice_must_be_in_the_entrypoint_account_position() {
    let report=pinocchio_source_result("pinocchio::program_entrypoint!(process_instruction); fn process_instruction(id: &Address, data: &[u8], extra: &mut [AccountView]) {}");
    assert_eq!(
        report
            .results
            .iter()
            .find(|r| r.rule_id == "SC104")
            .unwrap()
            .outcome,
        solcompat_core::Outcome::Unknown
    );
    let report=pinocchio_source_result("#![cfg(feature=\"variant\")]\npinocchio::program_entrypoint!(process_instruction); fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}");
    assert_eq!(
        report
            .results
            .iter()
            .find(|r| r.rule_id == "SC104")
            .unwrap()
            .outcome,
        solcompat_core::Outcome::Unknown
    );
}

#[test]
fn discovery_finds_conditional_entrypoint_modules_without_asserting_source_selection() {
    for declaration in [
        "#[cfg(not(feature=\"no-entrypoint\"))] mod entrypoint;",
        "#[cfg_attr(feature=\"disabled\",cfg(any()))] mod entrypoint;",
        "#[cfg(feature=\"program\")] mod outer { mod entrypoint; }",
    ] {
        let dir = ProjectDir::new();
        dir.write("Cargo.toml","[package]\nname='example'\nversion='0.1.0'\n[lib]\ncrate-type=['cdylib']\n[dependencies]\npinocchio='0.11'\n");
        dir.write("Cargo.lock","version=3\n[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['pinocchio']\n[[package]]\nname='pinocchio'\nversion='0.11.2'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n");
        dir.write("src/lib.rs", declaration);
        let path = if declaration.contains("outer") {
            "src/outer/entrypoint.rs"
        } else {
            "src/entrypoint.rs"
        };
        dir.write(path,"pinocchio::entrypoint!(process_instruction); fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}");
        let collected = dir.collect(None);
        assert_eq!(collected.project.programs.len(), 1, "{declaration}");
        assert_eq!(collected.project.programs[0].framework, "pinocchio");
        let report = solcompat_core::check(
            collected.project,
            &solcompat_core::Dataset::bundled().unwrap(),
            solcompat_core::Policy::default(),
            &[],
            &solcompat_core::AnalysisTarget::named("local").unwrap(),
        )
        .unwrap();
        assert_eq!(
            report
                .results
                .iter()
                .find(|r| r.rule_id == "SC104")
                .unwrap()
                .outcome,
            if declaration.contains("no-entrypoint") {
                solcompat_core::Outcome::Pass
            } else {
                solcompat_core::Outcome::Unknown
            }
        );
        assert_eq!(
            report.project.programs[0]
                .evidence
                .iter()
                .any(|ev| ev.kind == "unresolved-rust"),
            !declaration.contains("no-entrypoint")
        );
    }
}

#[test]
fn uncertain_sibling_module_does_not_hide_a_declared_entrypoint() {
    let dir = ProjectDir::new();
    dir.write(
        "Cargo.toml",
        "[package]\nname='example'\nversion='0.1.0'\n[dependencies]\npinocchio='0.11'\n",
    );
    dir.write("src/lib.rs", "mod missing; mod entrypoint;");
    dir.write(
        "src/entrypoint.rs",
        "pinocchio::entrypoint!(process_instruction);",
    );
    assert_eq!(dir.collect(None).project.programs.len(), 1);
    dir.write("src/lib.rs", "mod missing;");
    assert!(
        collect(&dir.0, None, None, None, &[]).is_err(),
        "an unlinked entrypoint file must not identify a program"
    );
}

#[test]
fn pinocchio_default_profile_selects_conventional_entrypoint_and_excludes_tests() {
    for (features, expected) in [
        ("no-entrypoint=[]", solcompat_core::Outcome::Pass),
        (
            "default=['cpi']\ncpi=['no-entrypoint']\nno-entrypoint=[]",
            solcompat_core::Outcome::Unknown,
        ),
        (
            "default=['no-entrypoint']\nno-entrypoint=[]",
            solcompat_core::Outcome::Unknown,
        ),
        (
            "default=['helper/no-entrypoint']\nno-entrypoint=[]",
            solcompat_core::Outcome::Pass,
        ),
        (
            "default=['dep:no-entrypoint']",
            solcompat_core::Outcome::Pass,
        ),
        (
            "default=['a']\na=['b']\nb=['a']\nno-entrypoint=[]",
            solcompat_core::Outcome::Pass,
        ),
        (
            "default='invalid'\nno-entrypoint=[]",
            solcompat_core::Outcome::Unknown,
        ),
    ] {
        let dir = ProjectDir::new();
        dir.write("Cargo.toml", &format!("[package]\nname='example'\nversion='0.1.0'\n[lib]\ncrate-type=['cdylib']\n[dependencies]\npinocchio='0.11'\n[features]\n{features}\n"));
        dir.write("Cargo.lock", "version=3\n[[package]]\nname='example'\nversion='0.1.0'\ndependencies=['pinocchio']\n[[package]]\nname='pinocchio'\nversion='0.11.2'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n");
        dir.write("src/lib.rs", "#[cfg(not(feature=\"no-entrypoint\"))] mod entrypoint; #[cfg(test)] mod missing_tests; #[cfg(test)] use pinocchio::account_info::AccountInfo;");
        dir.write("src/entrypoint.rs", "pinocchio::entrypoint!(process_instruction); fn process_instruction(id: &Address, accounts: &mut [AccountView], data: &[u8]) {}");
        let report = solcompat_core::check(
            dir.collect(None).project,
            &solcompat_core::Dataset::bundled().unwrap(),
            solcompat_core::Policy::default(),
            &[],
            &solcompat_core::AnalysisTarget::named("local").unwrap(),
        )
        .unwrap();
        let result = report
            .results
            .iter()
            .find(|r| r.rule_id == "SC104")
            .unwrap();
        assert_eq!(result.outcome, expected, "{features}");
        assert!(result.summary.contains("package default features"));
        assert!(!report.project.programs[0]
            .evidence
            .iter()
            .any(|ev| ev.kind == "pinocchio-account-info"));
    }
}

#[test]
fn conventional_gates_preserve_active_mismatches_and_unknown_target_conditions() {
    for (condition, reference, expected) in [
        (
            "not(feature=\"no-entrypoint\")",
            "&",
            solcompat_core::Outcome::Finding,
        ),
        (
            "all(not(test),not(feature=\"no-entrypoint\"))",
            "&mut",
            solcompat_core::Outcome::Pass,
        ),
        (
            "all(not(feature=\"no-entrypoint\"),target_os=\"solana\")",
            "&mut",
            solcompat_core::Outcome::Unknown,
        ),
    ] {
        let report = pinocchio_source_result(&format!("pinocchio::entrypoint!(process_instruction); #[cfg({condition})] fn process_instruction(id: &Address, accounts: {reference} [AccountView], data: &[u8]) {{}}"));
        assert_eq!(
            report
                .results
                .iter()
                .find(|r| r.rule_id == "SC104")
                .unwrap()
                .outcome,
            expected
        );
    }
}
