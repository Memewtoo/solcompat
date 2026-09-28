use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn fixture(name: &str) -> PathBuf {
    root().join("fixtures").join(name)
}
fn run(path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .arg("check")
        .arg("--path")
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}
fn report(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn rule<'a>(data: &'a Value, id: &str) -> &'a Value {
    data["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["rule_id"] == id)
        .unwrap()
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solcompat-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn copy_fixture(name: &str) -> Self {
        let temp = Self::new();
        for entry in fs::read_dir(fixture(name)).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), temp.0.join(entry.file_name())).unwrap();
        }
        temp
    }
    fn edit_config(&self, edit: impl FnOnce(String) -> String) {
        let path = self.0.join("solcompat.toml");
        fs::write(&path, edit(fs::read_to_string(&path).unwrap())).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failure_is_explained_without_claiming_decoder_analysis() {
    let output = run(&fixture("rpc-v1-failure"), &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    let finding = rule(&data, "SC201");
    assert_eq!(finding["rule_id"], "SC201");
    assert_eq!(finding["rule_name"], "RPC transaction-version acceptance");
    assert_eq!(finding["outcome"], "finding");
    assert_eq!(finding["subject"], "indexer/blocks");
    assert_eq!(data["counts"]["errors"], 1);
    assert!(finding["remediation"]["steps"][0]
        .as_str()
        .unwrap()
        .contains("SC201 checks request acceptance only"));
    assert!(finding["remediation"]["steps"][2]
        .as_str()
        .unwrap()
        .contains("does not fix the application"));
    assert!(data["dataset"]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

#[test]
fn detailed_changes_only_terminal_presentation() {
    let path = fixture("rpc-v1-failure");
    let compact = run(&path, &[]);
    let detailed = run(&path, &["--detailed"]);
    assert_eq!(compact.status.code(), detailed.status.code());
    let compact_text = String::from_utf8(compact.stdout).unwrap();
    let detailed_text = String::from_utf8(detailed.stdout).unwrap();
    assert!(!compact_text.contains("Example:"));
    assert!(detailed_text.contains("Example:"));
    assert!(detailed_text.contains("Reference:"));
    assert!(compact_text.contains("RPC transaction-version acceptance (SC201)"));
    assert!(compact_text.contains("1 error"));
    assert!(detailed_text.contains("1 error"));
    assert!(!compact_text.contains("Coverage:"));
    assert_eq!(
        run(&path, &["--format", "json"]).stdout,
        run(&path, &["--format", "json", "--detailed"]).stdout
    );
}

#[test]
fn terminal_colors_are_controllable_and_never_leak_into_json() {
    let path = fixture("rpc-v1-failure");
    let colored = run(&path, &["--color", "always"]);
    let colored = String::from_utf8(colored.stdout).unwrap();
    assert!(colored.contains("\x1b[1;31mERROR\x1b[0m"));
    assert!(colored.contains("\x1b[1;31mINCOMPATIBLE FOR CHECKED RULES\x1b[0m"));

    let plain = run(&path, &["--color", "never"]);
    assert!(!plain.stdout.contains(&0x1b));

    let json = run(&path, &["--format", "json", "--color", "always"]);
    assert!(!json.stdout.contains(&0x1b));
    report(&json);
}

#[test]
fn all_not_applicable_results_do_not_claim_compatibility() {
    let project = Temp::copy_fixture("rpc-unknown");
    fs::write(
        project.0.join("solcompat.toml"),
        "schema_version = 1\n[[clients]]\nid = \"client\"\nmanifest = \"package.json\"\nrequired_read_versions = []\n",
    )
    .unwrap();
    let output = run(&project.0, &["--detailed"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Verdict: NO APPLICABLE CHECKS"));
    assert!(text.contains("No configured checks apply"));
    assert!(!text.contains("Verdict: COMPATIBLE"));
    assert!(!text.contains("N/A"));
    assert!(!text.contains("not applicable"));
}

#[test]
fn discovered_client_without_rpc_evidence_does_not_activate_sc201() {
    let output = run(&fixture("rpc-unknown"), &["--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let data = report(&output);
    assert_eq!(data["counts"]["unknown"], 0);
    assert_eq!(data["counts"]["passed"], 0);
    assert_eq!(data["counts"]["not_applicable"], 1);
    assert!(data["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|result| result["rule_id"] != "SC201"));
    assert_eq!(
        run(&fixture("rpc-unknown"), &["--deny-unknown"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn configured_rpc_read_with_missing_requirements_remains_actionable() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    project.edit_config(|text| {
        text.replace(
            "required_read_versions = [\"legacy\", \"v0\", \"v1\"]\n",
            "",
        )
    });
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let data = report(&output);
    assert_eq!(rule(&data, "SC200")["outcome"], "pass");
    assert_eq!(rule(&data, "SC201")["outcome"], "unknown");
}

#[test]
fn empty_read_requirements_cannot_contradict_configured_rpc_reads() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    project.edit_config(|text| {
        text.replace(
            "required_read_versions = [\"legacy\", \"v0\", \"v1\"]",
            "required_read_versions = []",
        )
    });
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("cannot declare required_read_versions = []"));
}

#[test]
fn summary_reads_do_not_trigger_full_transaction_requirements() {
    let output = run(
        &fixture("rpc-summary"),
        &["--format", "json", "--deny-unknown"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report(&output)["results"][0]["outcome"], "not_applicable");
}

#[test]
fn pass_is_limited_to_the_request_contract() {
    let output = run(
        &fixture("rpc-v1-pass"),
        &["--format", "json", "--deny-unknown", "--deny-warnings"],
    );
    assert_eq!(output.status.code(), Some(0));
    let data = report(&output);
    assert_eq!(data["counts"]["passed"], 2);
    assert!(rule(&data, "SC201")["explanation"]
        .as_str()
        .unwrap()
        .contains("does not establish decoder support"));
}

#[test]
fn unverified_decoder_fork_does_not_match_registry_compatibility() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    let lock = project.0.join("package-lock.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&lock).unwrap()).unwrap();
    value["packages"]["node_modules/@solana/web3.js"]["resolved"] =
        "git+https://example.invalid/fork.git".into();
    fs::write(lock, value.to_string()).unwrap();
    let data = report(&run(&project.0, &["--format", "json"]));
    assert_eq!(rule(&data, "SC200")["outcome"], "unknown");
    assert_eq!(rule(&data, "SC201")["outcome"], "pass");
}

#[test]
fn unsupported_package_lock_format_is_an_input_error() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    let lock = project.0.join("package-lock.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&lock).unwrap()).unwrap();
    value["lockfileVersion"] = 1.into();
    fs::write(lock, value.to_string()).unwrap();
    assert_eq!(
        run(&project.0, &["--format", "json"]).status.code(),
        Some(2)
    );
}

#[test]
fn omission_is_not_the_same_as_missing_declaration() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    project.edit_config(|text| {
        text.replace(
            "max_supported_transaction_version = 1",
            "max_supported_transaction_version = \"omitted\"",
        )
    });
    assert_eq!(run(&project.0, &[]).status.code(), Some(1));
    project.edit_config(|text| text.replace("max_supported_transaction_version = \"omitted\"", ""));
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rule(&report(&output), "SC201")["outcome"], "unknown");
}

#[test]
fn malformed_and_unsupported_inputs_exit_two_instead_of_passing() {
    for suffix in [
        "\nunknown_key = true\n",
        "\n[[clients]]\nid = \"indexer\"\nmanifest = \"package.json\"\n",
    ] {
        let project = Temp::copy_fixture("rpc-v1-pass");
        project.edit_config(|text| text + suffix);
        let output = run(&project.0, &["--format", "json"]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(report(&output)["exit_code"], 2);
    }
    for value in ["-1", "256", "1.5", "\"1\"", "\"legacy\""] {
        let project = Temp::copy_fixture("rpc-v1-pass");
        project.edit_config(|text| {
            text.replace(
                "max_supported_transaction_version = 1",
                &format!("max_supported_transaction_version = {value}"),
            )
        });
        assert_eq!(run(&project.0, &[]).status.code(), Some(2), "{value}");
    }
    let project = Temp::copy_fixture("rpc-v1-pass");
    fs::write(project.0.join("package.json"), "{ invalid json }").unwrap();
    assert_eq!(run(&project.0, &[]).status.code(), Some(2));
    assert_eq!(run(&Temp::new().0, &[]).status.code(), Some(2));
    assert_eq!(
        run(&fixture("rpc-v1-pass"), &["--target", "mainnet"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&fixture("rpc-v1-pass"), &["--config", "missing.toml"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn unsupported_known_response_mode_is_unknown_not_incompatible() {
    let project = Temp::copy_fixture("rpc-v1-failure");
    project.edit_config(|text| {
        text.replace(
            "transaction_details = \"full\"",
            "transaction_details = \"accounts\"",
        )
    });
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rule(&report(&output), "SC201")["outcome"], "unknown");
}

#[test]
fn future_versions_remain_unknown() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    project.edit_config(|text| text.replace("\"v1\"", "\"v2\""));
    let output = run(&project.0, &["--format", "json", "--deny-unknown"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report(&output)["results"][0]["outcome"], "unknown");
}

#[test]
fn suppressions_stay_visible_and_do_not_become_passes() {
    let project = Temp::copy_fixture("rpc-v1-failure");
    project.edit_config(|text| text + "\n[[suppressions]]\nrule = \"SC201\"\nsubject = \"indexer/blocks\"\nreason = \"Tracked migration\"\n");
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let data = report(&output);
    assert_eq!(data["counts"]["suppressed"], 1);
    assert_eq!(data["counts"]["passed"], 1);
    assert_eq!(rule(&data, "SC201")["outcome"], "finding");
    assert!(String::from_utf8(run(&project.0, &[]).stdout)
        .unwrap()
        .contains("SUPPRESSED"));
    project.edit_config(|text| text.replace("indexer/blocks", "indexer/missing"));
    assert_eq!(run(&project.0, &[]).status.code(), Some(2));
}

#[test]
fn dataset_validation_is_on_the_real_cli_path() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    let path = project.0.join("data.json");
    let valid = fs::read_to_string(
        root().join("crates/solcompat-core/compatibility/records/rpc-read-contracts.json"),
    )
    .unwrap();
    fs::write(&path, &valid).unwrap();
    let with_data = run(
        &project.0,
        &["--format", "json", "--data", path.to_str().unwrap()],
    );
    assert_eq!(
        with_data.stdout,
        run(&project.0, &["--format", "json"]).stdout
    );
    let mut invalid: Value = serde_json::from_str(&valid).unwrap();
    invalid["records"][0]["sources"] = json!([]);
    fs::write(&path, invalid.to_string()).unwrap();
    let output = run(
        &project.0,
        &["--format", "json", "--data", path.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("source references"));
}

#[test]
fn inspection_is_inventory_not_a_compatibility_verdict() {
    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["inspect", "--format", "json", "--path"])
        .arg(fixture("rpc-v1-failure"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let data = report(&output);
    assert_eq!(data["command"], "inspect");
    assert_eq!(data["analysis_mode"], "inventory");
    assert_eq!(data["results"], json!([]));
    assert_eq!(
        data["project"]["clients"][0]["decoder_package"],
        "@solana/web3.js"
    );
}

#[test]
fn semantic_json_is_reproducible_across_directories_and_views() {
    let first = Temp::copy_fixture("rpc-v1-failure");
    let second = Temp::copy_fixture("rpc-v1-failure");
    assert_eq!(
        run(&first.0, &["--format", "json"]).stdout,
        run(&second.0, &["--format", "json", "--detailed"]).stdout
    );
    assert_eq!(
        run(&first.0, &["--format", "json"]).stdout,
        run(&first.0, &["--format", "json"]).stdout
    );
}

#[test]
fn mixed_workspace_exercises_phase_c_rule_boundaries() {
    let output = run(&fixture("mixed-workspace"), &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(data["project"]["programs"][0]["framework"], "anchor");
    for id in ["SC001", "SC003", "SC103"] {
        assert_eq!(rule(&data, id)["outcome"], "pass", "{id}");
    }
    assert_eq!(rule(&data, "SC302")["severity"], "warning");
    for id in ["SC200", "SC201", "SC400"] {
        assert_eq!(rule(&data, id)["severity"], "error", "{id}");
    }
}

fn sbpf_elf(flags: u32, machine: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[0..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    bytes[48..52].copy_from_slice(&flags.to_le_bytes());
    bytes
}

#[test]
fn supplied_artifact_uses_explicit_conditional_policy() {
    let project = Temp::new();
    fs::write(project.0.join("program.so"), sbpf_elf(2, 247)).unwrap();
    fs::write(project.0.join("solcompat.toml"), "schema_version = 1\n[[artifacts]]\nid = \"vault\"\npath = \"program.so\"\noperation = \"upgrade\"\n").unwrap();
    let local = report(&run(&project.0, &["--format", "json"]));
    assert_eq!(rule(&local, "SC102")["outcome"], "unknown");
    let scenario = run(
        &project.0,
        &["--format", "json", "--target", "future-sbpfv3-deployment"],
    );
    assert_eq!(scenario.status.code(), Some(1));
    let scenario = report(&scenario);
    assert_eq!(rule(&scenario, "SC102")["outcome"], "finding");
    assert_eq!(rule(&scenario, "SC102")["conditional"], true);
    fs::write(project.0.join("program.so"), sbpf_elf(3, 247)).unwrap();
    let pass = report(&run(
        &project.0,
        &["--format", "json", "--target", "future-sbpfv3-deployment"],
    ));
    assert_eq!(rule(&pass, "SC102")["outcome"], "pass");

    // Newer cargo-build-sbf toolchains use the dedicated EM_SBPF value 263.
    fs::write(project.0.join("program.so"), sbpf_elf(3, 263)).unwrap();
    let em_sbpf = report(&run(
        &project.0,
        &["--format", "json", "--target", "future-sbpfv3-deployment"],
    ));
    assert_eq!(rule(&em_sbpf, "SC102")["outcome"], "pass");

    fs::write(project.0.join("solcompat.toml"), "schema_version = 1\n[[artifacts]]\nid = \"vault\"\npath = \"program.so\"\noperation = \"execute\"\n").unwrap();
    let execute = report(&run(
        &project.0,
        &["--format", "json", "--target", "future-sbpfv3-deployment"],
    ));
    assert_eq!(rule(&execute, "SC102")["outcome"], "not_applicable");
    fs::write(project.0.join("solcompat.toml"), "schema_version = 1\n[[artifacts]]\nid = \"vault\"\npath = \"program.so\"\noperation = \"upgrade\"\n").unwrap();

    let target = project.0.join("target.json");
    fs::write(&target, json!({"schema_version":1,"id":"review-v2","conditional":true,"artifact_policies":[{"loader":"loader-v3","operations":["upgrade"],"allowed_sbpf_versions":["v2"]}]}).to_string()).unwrap();
    fs::write(project.0.join("program.so"), sbpf_elf(2, 247)).unwrap();
    let custom = report(&run(
        &project.0,
        &[
            "--format",
            "json",
            "--target-file",
            target.to_str().unwrap(),
        ],
    ));
    assert_eq!(custom["target"], "review-v2");
    assert!(custom["target_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(rule(&custom, "SC102")["outcome"], "pass");
}

#[test]
fn unsupported_elf_machine_is_an_input_error() {
    let project = Temp::new();
    fs::write(project.0.join("program.so"), sbpf_elf(3, 62)).unwrap();
    fs::write(
        project.0.join("solcompat.toml"),
        "schema_version = 1\n[[artifacts]]\nid = \"bad\"\npath = \"program.so\"\n",
    )
    .unwrap();
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not EM_BPF (247) or EM_SBPF (263)"));
}

#[test]
fn captured_cargo_metadata_resolves_an_ambiguous_lockfile() {
    let project = Temp::new();
    fs::write(project.0.join("Cargo.toml"), "[package]\nname = \"vault\"\nversion = \"0.1.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\nanchor-lang = \"0.31.1\"\n").unwrap();
    fs::write(project.0.join("Cargo.lock"), "version = 3\n[[package]]\nname = \"anchor-lang\"\nversion = \"0.30.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n[[package]]\nname = \"anchor-lang\"\nversion = \"0.31.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n").unwrap();
    fs::write(project.0.join("solcompat.toml"), "schema_version = 1\n[[programs]]\nid = \"vault\"\nmanifest = \"Cargo.toml\"\nanchor_cli_version = \"0.31.1\"\n").unwrap();
    let manifest = project.0.join("Cargo.toml").canonicalize().unwrap();
    let metadata = json!({
        "version": 1,
        "packages": [
            {"id":"vault 0.1.0", "name":"vault", "version":"0.1.0", "manifest_path":manifest},
            {"id":"anchor-lang 0.31.1", "name":"anchor-lang", "version":"0.31.1", "source":"registry+https://github.com/rust-lang/crates.io-index", "manifest_path":"/registry/anchor-lang/Cargo.toml"}
        ],
        "resolve": {"nodes":[{"id":"vault 0.1.0", "deps":[{"name":"anchor_lang", "pkg":"anchor-lang 0.31.1"}]}]}
    });
    fs::write(project.0.join("metadata.json"), metadata.to_string()).unwrap();
    let before = report(&run(&project.0, &["--format", "json"]));
    assert_eq!(rule(&before, "SC003")["outcome"], "unknown");
    let after = report(&run(
        &project.0,
        &[
            "--format",
            "json",
            "--cargo-metadata",
            project.0.join("metadata.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(rule(&after, "SC003")["outcome"], "pass");
}

#[test]
fn cargo_compatible_anchor_resolution_passes_with_an_advisory_warning() {
    let project = Temp::new();
    fs::create_dir(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/lib.rs"), "pub fn program() {}\n").unwrap();
    fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"anchor-range\"\nversion = \"0.1.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nanchor-lang = \"1.0.2\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("Cargo.lock"),
        "version = 3\n\n[[package]]\nname = \"anchor-range\"\nversion = \"0.1.0\"\ndependencies = [\n \"anchor-lang\",\n]\n\n[[package]]\nname = \"anchor-lang\"\nversion = \"1.2.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("solcompat.toml"),
        "schema_version = 1\n\n[[programs]]\nid = \"anchor-range\"\nmanifest = \"Cargo.toml\"\nanchor_cli_version = \"1.0.2\"\n",
    )
    .unwrap();

    let output = run(&project.0, &["--detailed"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Verdict: PASSED WITH WARNINGS"));
    assert!(text.contains("Cargo accepted anchor-lang 1.2.0 for requirement 1.0.2"));
    assert!(text.contains("No incompatibility was detected"));

    let strict = run(&project.0, &["--deny-warnings"]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(String::from_utf8(strict.stdout)
        .unwrap()
        .contains("Verdict: FAILED POLICY — WARNINGS DENIED"));
}

#[cfg(unix)]
#[test]
fn opt_in_probes_refuse_scripts_without_executing_them() {
    use std::os::unix::fs::PermissionsExt;
    let project = Temp::copy_fixture("rpc-unknown");
    let bin = project.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = project.0.join("PROBE_RAN");
    for tool in ["anchor", "cargo-build-sbf", "rustc"] {
        let path = bin.join(tool);
        fs::write(
            &path,
            format!("#!/bin/sh\ntouch '{}'\necho 9.9.9\n", marker.display()),
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["inspect", "--format", "json", "--probe-tools", "--path"])
        .arg(&project.0)
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(!marker.exists());
    let data = report(&output);
    assert!(data["project"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|tool| tool["status"]
            .as_str()
            .unwrap()
            .contains("refusing a script")));
}

#[test]
fn explicit_config_paths_are_resolved_relative_to_the_config_file() {
    let project = Temp::copy_fixture("rpc-v1-pass");
    fs::create_dir(project.0.join("config")).unwrap();
    let original = fs::read_to_string(project.0.join("solcompat.toml")).unwrap();
    let config = project.0.join("config/my project.toml");
    fs::write(
        &config,
        original.replace(
            "manifest = \"package.json\"",
            "manifest = \"../package.json\"",
        ),
    )
    .unwrap();
    let output = run(
        &project.0,
        &[
            "--config",
            config.to_str().unwrap(),
            "--target",
            "local",
            "--deny-unknown",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("my project.toml'"));
    assert!(text.contains("--target local --deny-unknown --detailed"));
}

#[test]
fn project_scripts_are_not_executed_and_inputs_are_unchanged() {
    let project = Temp::copy_fixture("rpc-v1-failure");
    let package = project.0.join("package.json");
    let mut data: Value = serde_json::from_str(&fs::read_to_string(&package).unwrap()).unwrap();
    data["scripts"] =
        json!({"preinstall": "touch SHOULD_NOT_EXIST", "build": "touch SHOULD_NOT_EXIST"});
    fs::write(&package, data.to_string()).unwrap();
    let before: BTreeMap<_, _> = fs::read_dir(&project.0)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), fs::read(entry.path()).unwrap())
        })
        .collect();
    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["check", "--format", "json", "--path"])
        .arg(&project.0)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let after: BTreeMap<_, _> = fs::read_dir(&project.0)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), fs::read(entry.path()).unwrap())
        })
        .collect();
    assert_eq!(before, after);
}

#[test]
fn no_subcommand_checks_the_current_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .current_dir(fixture("rpc-v1-failure"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("RPC transaction-version acceptance (SC201)"));
    assert!(text.contains("check --detailed"));
}

#[test]
fn cdylib_client_without_an_entrypoint_is_not_a_program() {
    let project = Temp::new();
    let client = project.0.join("clients/rust");
    let program = project.0.join("programs/native");
    fs::create_dir_all(client.join("src")).unwrap();
    fs::create_dir_all(program.join("src")).unwrap();
    fs::write(
        client.join("Cargo.toml"),
        r#"[package]
name = "generated-client"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["cdylib", "lib"]
[dependencies]
anchor = { package = "anchor-lang", version = "0.31.1", optional = true }
solana-program = "3"
"#,
    )
    .unwrap();
    fs::write(client.join("src/lib.rs"), "pub fn client() {}\n").unwrap();
    fs::write(
        program.join("Cargo.toml"),
        r#"[package]
name = "real-program"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["cdylib", "lib"]
[dependencies]
solana-program = "3"
"#,
    )
    .unwrap();
    fs::write(
        program.join("src/lib.rs"),
        "solana_program::entrypoint!(process_instruction);\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["inspect", "--format", "json", "--path"])
        .arg(&project.0)
        .output()
        .unwrap();
    let data = report(&output);
    let programs = data["project"]["programs"].as_array().unwrap();
    assert_eq!(programs.len(), 1);
    assert_eq!(programs[0]["manifest"], "programs/native/Cargo.toml");
    assert_eq!(programs[0]["framework"], "native");
}

#[cfg(unix)]
#[test]
fn symlink_manifest_cannot_silently_escape_project() {
    let project = Temp::new();
    std::os::unix::fs::symlink(
        fixture("rpc-v1-pass")
            .canonicalize()
            .unwrap()
            .join("package.json"),
        project.0.join("package.json"),
    )
    .unwrap();
    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(report(&output)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("outside selected project root"));
}

#[test]
fn direct_rpc_source_activates_transaction_v1_checks_without_config() {
    let project = Temp::new();
    fs::create_dir(project.0.join("src")).unwrap();
    fs::write(
        project.0.join("package.json"),
        r#"{"name":"reader","dependencies":{"@solana/kit":"^7.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        project.0.join("src/index.ts"),
        r#"export async function read(rpc: any, slot: bigint) {
  return rpc.getBlock(slot, {
    encoding: "json",
    transactionDetails: "full",
    maxSupportedTransactionVersion: 0,
  });
}
"#,
    )
    .unwrap();

    let output = run(&project.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(rule(&data, "SC200")["outcome"], "finding");
    assert!(rule(&data, "SC200")["title"]
        .as_str()
        .unwrap()
        .contains("range excludes"));
    assert_eq!(rule(&data, "SC201")["outcome"], "finding");
    assert!(rule(&data, "SC201")["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| evidence["path"] == "src/index.ts"
            && evidence["pointer"]
                .as_str()
                .unwrap()
                .starts_with("/source:")));
}

#[test]
fn nested_program_uses_ancestor_workspace_requirements_and_lockfile() {
    let workspace = Temp::new();
    let program = workspace.0.join("programs/counter");
    fs::create_dir_all(program.join("src")).unwrap();
    fs::write(
        workspace.0.join("Cargo.toml"),
        "[workspace]\nmembers = [\"programs/counter\"]\nresolver = \"2\"\n[workspace.dependencies]\nanchor-lang = \"0.31.1\"\n",
    )
    .unwrap();
    fs::write(
        workspace.0.join("Cargo.lock"),
        "version = 3\n[[package]]\nname = \"anchor-lang\"\nversion = \"0.31.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    )
    .unwrap();
    fs::write(
        program.join("Cargo.toml"),
        "[package]\nname = \"counter\"\nversion = \"0.1.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\nanchor-lang.workspace = true\n",
    )
    .unwrap();
    fs::write(
        program.join("src/lib.rs"),
        "use anchor_lang::prelude::*;\nsolana_program::entrypoint!(process_instruction);\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["inspect", "--format", "json", "--path"])
        .arg(&program)
        .output()
        .unwrap();
    let data = report(&output);
    assert_eq!(
        data["project"]["programs"][0]["dependencies"]["dependencies/anchor-lang"],
        "0.31.1"
    );
    assert_eq!(
        data["project"]["programs"][0]["resolved_dependencies"]["anchor-lang"],
        "0.31.1"
    );
}

#[test]
fn upgrade_advisor_reports_only_crossed_anchor_boundaries() {
    let project = Temp::new();
    fs::create_dir(project.0.join("src")).unwrap();
    fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"legacy-anchor\"\nversion = \"0.1.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\nanchor-lang = \"0.30.1\"\nsolana-program = \"1.18\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("Cargo.lock"),
        "version = 3\n[[package]]\nname = \"anchor-lang\"\nversion = \"0.30.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("src/lib.rs"),
        r#"use anchor_lang::prelude::*;
fn migration_examples(ctx: Context<Example>, cpi_accounts: ExampleCpi) {
    let _ = Example::discriminator();
    let _ = CpiContext::new(ctx.accounts.program.to_account_info(), cpi_accounts);
}
"#,
    )
    .unwrap();
    fs::write(
        project.0.join("Anchor.toml"),
        "[toolchain]\nanchor_version = \"0.30.1\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("package.json"),
        r#"{"name":"legacy-client","dependencies":{"@coral-xyz/anchor":"^0.30.1"}}"#,
    )
    .unwrap();
    fs::write(
        project.0.join("solcompat.toml"),
        "schema_version = 1\n[[programs]]\nid = \"legacy-anchor\"\nmanifest = \"Cargo.toml\"\nanchor_cli_version = \"0.30.1\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args([
            "upgrade", "--format", "json", "--anchor", "1.2.0", "--agave", "4.3.0", "--path",
        ])
        .arg(&project.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(data["command"], "upgrade");
    for id in [
        "UP101", "UP102", "UP103", "UP104", "UP105", "UP106", "UP107",
    ] {
        assert!(
            data["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["rule_id"] == id),
            "missing {id}"
        );
    }
    assert_eq!(rule(&data, "UP104")["severity"], "error");
    assert!(!data["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|result| result["rule_id"] == "UP200"));
}

#[cfg(unix)]
#[test]
fn explicit_build_runs_the_detected_program_build() {
    use std::os::unix::fs::PermissionsExt;
    let project = Temp::new();
    fs::create_dir(project.0.join("src")).unwrap();
    fs::write(
        project.0.join("Cargo.toml"),
        "[package]\nname = \"pin-program\"\nversion = \"0.1.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\npinocchio = \"0.11\"\n",
    )
    .unwrap();
    fs::write(
        project.0.join("src/lib.rs"),
        "pinocchio::entrypoint!(process_instruction);\n",
    )
    .unwrap();
    let bin = project.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = project.0.join("BUILD_RAN");
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n: > '{}'\necho fake SBF build succeeded\n",
            marker.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cargo, permissions).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_solcompat"))
        .args(["--build", "check", "--format", "json", "--path"])
        .arg(&project.0)
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(marker.exists());
    let data = report(&output);
    assert_eq!(rule(&data, "SC100")["outcome"], "pass");
}

#[test]
fn pinocchio_source_syntax_is_checked_against_resolved_release() {
    fn project(version: &str, source: &str) -> Temp {
        let project = Temp::new();
        fs::create_dir(project.0.join("src")).unwrap();
        fs::write(
            project.0.join("Cargo.toml"),
            format!(
                "[package]\nname = \"pin-syntax\"\nversion = \"0.1.0\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\npinocchio = \"{version}\"\n"
            ),
        )
        .unwrap();
        fs::write(
            project.0.join("Cargo.lock"),
            format!(
                "version = 3\n[[package]]\nname = \"pinocchio\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
            ),
        )
        .unwrap();
        fs::write(project.0.join("src/lib.rs"), source).unwrap();
        project
    }

    let v009 = project(
        "0.9.2",
        "use pinocchio::{entrypoint, account_info::AccountInfo, pubkey::Pubkey, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let data = report(&run(&v009.0, &["--format", "json"]));
    assert_eq!(rule(&data, "SC104")["severity"], "warning");
    assert!(rule(&data, "SC104")["title"]
        .as_str()
        .unwrap()
        .contains("0.9 crosses two"));
    assert!(rule(&data, "SC104")["summary"]
        .as_str()
        .unwrap()
        .contains("Migrating to 0.10"));

    let v009_with_new_syntax = project(
        "0.9.2",
        "use pinocchio::{entrypoint, Address, AccountView, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Address, accounts: &[AccountView], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let output = run(&v009_with_new_syntax.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(rule(&data, "SC104")["severity"], "error");
    assert!(rule(&data, "SC104")["title"]
        .as_str()
        .unwrap()
        .contains("0.10+ AccountView"));

    let v010_with_legacy_syntax = project(
        "0.10.2",
        "use pinocchio::{entrypoint, account_info::AccountInfo, pubkey::Pubkey, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let output = run(&v010_with_legacy_syntax.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(rule(&data, "SC104")["severity"], "error");
    assert!(rule(&data, "SC104")["title"]
        .as_str()
        .unwrap()
        .contains("removed 0.9 account types"));

    let pre_011 = project(
        "0.10.1",
        "use pinocchio::{entrypoint, Address, AccountView, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Address, accounts: &[AccountView], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let data = report(&run(&pre_011.0, &["--format", "json"]));
    assert_eq!(rule(&data, "SC104")["severity"], "warning");
    assert!(rule(&data, "SC104")["summary"]
        .as_str()
        .unwrap()
        .contains("&mut [AccountView]"));

    let current = project(
        "0.11.2",
        "use pinocchio::{entrypoint, Address, AccountView, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let data = report(&run(&current.0, &["--format", "json"]));
    assert_eq!(rule(&data, "SC104")["outcome"], "pass");

    let legacy = project(
        "0.11.2",
        "use pinocchio::{entrypoint, account_info::AccountInfo, pubkey::Pubkey, ProgramResult};\nentrypoint!(process_instruction);\nfn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult { Ok(()) }\n",
    );
    let output = run(&legacy.0, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let data = report(&output);
    assert_eq!(rule(&data, "SC104")["severity"], "error");
    assert!(rule(&data, "SC104")["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| evidence["pointer"] == "/source:1"));
}
