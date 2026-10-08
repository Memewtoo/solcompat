# How SolCompat reaches a result

SolCompat reads project files, turns them into an inventory, and evaluates that inventory against reviewed compatibility rules. The models in `solcompat-core` describe the inputs and outputs of those checks. Their constructors live in `solcompat-project`; they do not need an `impl` in the model file.

If you are looking at `Program` and wondering where its values come from, start with [`collect_program()`](../crates/solcompat-project/src/program.rs). It creates the initial record. [`collect()`](../crates/solcompat-project/src/collect.rs) adds configured tool selections, imported resolution, and Anchor.toml information. The CLI can then request probes and call `enrich_tool_observations()` in the project crate to fill missing tool selections.

## The pipeline

```text
CLI options and optional solcompat.toml
    -> configuration bindings and manifest discovery
    -> Cargo/npm resolution and source observations
    -> enrichment and sorted project inventory
    -> inspect: inventory only
       check: current-state rules
       upgrade: migration advice for selected versions
    -> optional build, then fresh collection and check evaluation (check only)
    -> report counts and exit policy
    -> JSON or terminal output
```

There are three crates, with dependencies pointing toward the core:

- `solcompat` owns command arguments, application sequencing, and rendering.
- `solcompat-project` owns file reads, component construction, dependency resolution, source observations, and opt-in tool execution. It depends on the core models.
- `solcompat-core` owns models, reviewed data, current-state and upgrade evaluation, rule identities, and report policy. It does not depend on the collector or CLI.

Collection does not execute Cargo, Anchor, npm, package scripts, or RPC requests. Tool probes require `--probe-tools`; building requires `--build`. These options provide additional observations rather than proof that every future ecosystem change is safe.

## Where to look

Paths in this table are relative to the repository root.

| Responsibility | Code |
|---|---|
| Argument definitions, application sequencing, and output | [cli.rs](../src/cli.rs), [app.rs](../src/app.rs), [main.rs](../src/main.rs) |
| Public collection API and re-exports | [project/src/lib.rs](../crates/solcompat-project/src/lib.rs) |
| Collection order, binding validation, enrichment | [project/src/collect.rs](../crates/solcompat-project/src/collect.rs) |
| Configuration shapes and small validation helpers | [project/src/config.rs](../crates/solcompat-project/src/config.rs) |
| Bounded reads, containment, labels, evidence | [project/src/input.rs](../crates/solcompat-project/src/input.rs) |
| Traversal, exclusions, automatic IDs | [project/src/discovery.rs](../crates/solcompat-project/src/discovery.rs) |
| Program construction and framework identification | [project/src/program.rs](../crates/solcompat-project/src/program.rs) |
| Client construction and identification | [project/src/client.rs](../crates/solcompat-project/src/client.rs) |
| Cargo declarations, workspace inheritance, lockfiles, captured metadata | [project/src/cargo.rs](../crates/solcompat-project/src/cargo.rs) |
| npm declarations and lockfiles | [project/src/npm.rs](../crates/solcompat-project/src/npm.rs) |
| Rust entrypoints and framework syntax signals | [project/src/source/rust.rs](../crates/solcompat-project/src/source/rust.rs) |
| JS/TS tokenization and direct RPC calls | [javascript.rs](../crates/solcompat-project/src/source/javascript.rs), [rpc.rs](../crates/solcompat-project/src/source/rpc.rs) |
| Bound IDLs and existing ELF headers | [idl.rs](../crates/solcompat-project/src/idl.rs), [artifact.rs](../crates/solcompat-project/src/artifact.rs) |
| Probes and builds | [tools.rs](../crates/solcompat-project/src/tools.rs), [execution.rs](../crates/solcompat-project/src/execution.rs) |
| Serialized inputs and results | [core/src/model.rs](../crates/solcompat-core/src/model.rs) |
| Reviewed dataset validation and identity | [core/src/data.rs](../crates/solcompat-core/src/data.rs) |
| Current-state rule dispatch | [core/src/rules/mod.rs](../crates/solcompat-core/src/rules/mod.rs) |
| Compiler and sBPF build prerequisites | [rules/program.rs](../crates/solcompat-core/src/rules/program.rs) |
| Anchor and Pinocchio source/version alignment | [rules/anchor.rs](../crates/solcompat-core/src/rules/anchor.rs), [rules/pinocchio.rs](../crates/solcompat-core/src/rules/pinocchio.rs) |
| RPC acceptance and decoder capability | [rules/rpc.rs](../crates/solcompat-core/src/rules/rpc.rs), [rules/decoder.rs](../crates/solcompat-core/src/rules/decoder.rs) |
| IDL and artifact compatibility | [rules/idl.rs](../crates/solcompat-core/src/rules/idl.rs), [rules/artifact.rs](../crates/solcompat-core/src/rules/artifact.rs) |
| Rule identities and metadata ownership | [catalog.rs](../crates/solcompat-core/src/catalog.rs) |
| Suppressions, counts, exits, semantic verdicts | [report.rs](../crates/solcompat-core/src/report.rs) |
| Shared evaluation helpers and version parsing | [evaluation.rs](../crates/solcompat-core/src/evaluation.rs), [version.rs](../crates/solcompat-core/src/version.rs) |
| Target policy parsing | [target.rs](../crates/solcompat-core/src/target.rs) |
| Typed signals and resolution adapter | [signals.rs](../crates/solcompat-core/src/signals.rs), [project/resolution.rs](../crates/solcompat-project/src/resolution.rs) |
| Upgrade dispatch and release-family evaluators | [upgrade/mod.rs](../crates/solcompat-core/src/upgrade/mod.rs), [upgrade/anchor.rs](../crates/solcompat-core/src/upgrade/anchor.rs), [upgrade/agave.rs](../crates/solcompat-core/src/upgrade/agave.rs) |
| Terminal wording and colors | [src/render.rs](../src/render.rs) |

The project modules are private. The crate continues to expose `collect`, `Collected`, `read_text`, `probe_tools`, `build_program`, and `BuildObservation`. It also exposes `enrich_tool_observations` and `build_program_with_timeout` as additive helpers. Consumers do not need to know the internal file layout.

## Component identification and paths

Automatic program discovery requires a Cargo package with a recognized entrypoint and supported framework dependency or a `cdylib` library. Recognized parsed constructs include `entrypoint!`, `program_entrypoint!`, and `#[program]`. Comments and string literals do not count. Discovery follows declared default-path modules even when they have `cfg` or `cfg_attr`, so an entrypoint in conditionally included code identifies a candidate. A missing or conditional sibling module does not hide other declared paths. This does not prove the selected build enables that entrypoint: source evaluation applies the bounded default profile described below. Custom module paths remain outside automatic traversal. An explicit program binding bypasses the automatic entrypoint test, but still needs supported program evidence.

Framework selection checks required Anchor dependencies first, then required Pinocchio dependencies, then native Solana dependencies, then `cdylib`. Optional, build-only, or target-only framework dependencies alone do not identify an Anchor or Pinocchio program.

Automatic npm clients declare an `@solana/` package, `@coral-xyz/anchor`, or an `@anchor-lang/` package. An explicit binding can collect another package. SDK dependencies alone do not establish transaction-reading behavior.

Discovery visits sorted directory entries and skips `.git`, `.anchor`, `.next`, `build`, `coverage`, `dist`, `node_modules`, and `target`. It does not traverse symlink directories. Matching symlink manifests must resolve inside the selected root. JS/TS source traversal skips symlinks. Declared Rust library and module files are canonicalized and must remain inside the selected root. Explicit manifests, configuration, metadata, IDLs, and artifacts also undergo containment checks.

Text files have a 4 MiB limit; ELF files have a 16 MiB limit. Reads enforce limits both before and during reading. The public `read_text` helper itself does not enforce containment: its callers do so when appropriate. The CLI also uses it for explicitly selected dataset and target documents.

There is one intentional exception to project containment: nested Cargo projects can use ancestor workspace manifests and Cargo.lock. npm lockfile search stops at the selected root. Neither resolver installs dependencies.

## Field sources

The tables describe the current implementation, including missing-evidence behavior. A declaration such as `"1.0.2"` in Cargo.toml is a requirement, not evidence that the selected dependency is exactly 1.0.2. Absent optional values remain `None` unless a later stage supplies them.

### Program

| Field | Source and behavior |
|---|---|
| `id` | Explicit program binding, or automatic ID derived from the relative manifest path. Collisions fail rather than silently merge. |
| `name` | Cargo `[package].name`; falls back to the component ID. |
| `manifest` | Canonicalized manifest represented as a path relative to the selected root. |
| `framework` | Dependency and library identification in `collect_program`, as described above. |
| `candidate_reason` | Existing description based on `cdylib` presence or a framework dependency; it is not a complete entrypoint proof. |
| `rust_version` | Literal string from `[package].rust-version`; absent or inherited table values are not resolved here. |
| `dependencies` | Declared dependency requirements, with section/name keys and alias identity where supplied. Includes build and target dependencies. Workspace requirements are substituted when available. |
| `resolved_dependencies` | Normal required dependency edges from the matching local package in Cargo.lock. Package identity, canonical crates.io source, version requirement, and edge must agree. Captured metadata replaces the entire map for a matching manifest, including with an empty map; only normal, unconditional registry edges satisfying current normal dependency requirements qualify. Ambiguity stays unresolved. |
| `anchor_cli_version` | Explicit program configuration, then root Anchor.toml `[toolchain].anchor_version`, then optional CLI probe; later sources only fill missing values. |
| `sbf_rust_version` | Explicit program configuration only. Host rustc is not substituted. |
| `sbpf_arch` | Explicit program configuration only; not inferred from installed tools. |
| `platform_tools_version` | Explicit program configuration only. |
| `cargo_build_sbf_version` | Explicit program configuration, then an optional CLI probe if absent. |
| `evidence` | Manifest, inherited workspace declarations, lockfile or imported metadata, supported Rust source signals, configured bindings, and Anchor.toml observations, each with its own digest. Ancestor resolution paths use an `external-workspace:` label. |

### Client

| Field | Source and behavior |
|---|---|
| `id` | Explicit client binding, or automatic ID derived from its relative manifest path. |
| `name` | package.json `name`, otherwise absent. |
| `manifest` | Canonicalized relative package.json path. |
| `dependencies` | Declared dependencies, devDependencies, peerDependencies, and optionalDependencies, keyed by section/package. |
| `resolved_dependencies` | Nearest package-lock.json within the selected root, version 2 or 3; only supported registry entries for declared packages are retained. The nearest matching node_modules entry wins, including nested installations. Unsupported entries do not fall through to a different version. |
| `decoder_package` | Explicit binding when provided. Otherwise inferred from the SDK constructor bound to discovered reads. The package must be declared. Mixed SDK providers need an explicit binding; package priority never chooses the decoder. |
| `required_read_versions` | Explicit binding, sorted and validated. Otherwise reads bound to a supported SDK supply `legacy`, `v0`, and `v1`. Absent when neither supplies a contract. |
| `rpc_reads` | Configured reads, or source-discovered direct calls if there are no configured reads. Configured reads replace the need for source discovery for that client; they are not merged with discovered reads. |
| `evidence` | package.json digest, configured binding evidence, and source references for discovered calls. Lockfile observations carry their own path, pointer, and digest. |

`required_read_versions = []` with configured reads or a recognized source read is rejected as contradictory input. No source match is not proof that a client never reads transactions.

### RpcRead

| Field | Source and behavior |
|---|---|
| `id` | Explicit read ID, configured `read-N` fallback, or discovered `source-N` sequence. |
| `method` | Validated configured `getBlock`/`getTransaction`, or recognized direct method text. |
| `encoding` | Explicit configured value. Discovered calls without options or with literal options default to `json`; nonliteral options leave it unresolved. |
| `transaction_details` | Configured getBlock value. Discovered getBlock calls without options or with literal options default to `full`; getTransaction leaves it absent. |
| `max_supported_transaction_version` | Configured integer or `"omitted"`; missing configuration stays unresolved. The scanner recognizes numeric literal properties, treats missing properties in literal/no options as omitted, and leaves nonliteral option objects unresolved. |

An RPC maximum describes what versions the request accepts. It does not prove the SDK can decode those versions. Request checks and decoder checks use separate evidence.

Rust uses a syntax tree; JS/TS uses a bounded token scanner. Both ignore comments and string contents. JS/TS binds supported named SDK imports and unique local constructor assignments; it does not perform complete scope or type analysis. Dynamic expressions and unbound receivers remain unresolved. Read [supported inputs](SUPPORTED_INPUTS.md) and [limitations](LIMITATIONS.md) before expanding a detector's claims.

### IDLs, artifacts, and tools

| Model / field | Source and behavior |
|---|---|
| `IdlInput.id` | Explicit binding. |
| `IdlInput.path` | Bound file's canonicalized relative path. |
| `IdlInput.client` | Explicit binding to an existing collected client. |
| `IdlInput.reader_package` | Explicit binding; the package must be declared by that client. |
| `IdlInput.schema` | `metadata.spec` produces `anchor-spec-...`; otherwise presence of version and instructions produces `anchor-legacy`; otherwise `unknown`. |
| `IdlInput.evidence` | IDL byte digest and observed root pointer. |
| `Artifact.id` | Explicit binding, or `artifact-N` for additional CLI artifacts. |
| `Artifact.path` | Canonicalized relative ELF path. |
| `Artifact.program` | Optional configured program ID, validated against the inventory; absent for additional CLI artifacts. |
| `Artifact.loader` | Configured loader or `loader-v3` default. |
| `Artifact.operation` | Configured operation or `upgrade` default. |
| `Artifact.elf_machine` | Little-endian ELF header; accepted machine IDs are 247 and 263. |
| `Artifact.elf_flags` | Header flags read according to ELF class. |
| `Artifact.sbpf_version` | Flags 0–3 map to v0–v3; other flags remain unresolved. Machine identity is separate from version. |
| `Artifact.evidence` | Digest of the entire artifact with `/elf-header` pointer. |
| `ToolObservation.tool` | One of the requested built-in probes: anchor, cargo-build-sbf, rustc. |
| `ToolObservation.version` | Parsed token from executable stdout, or absent on unavailable/unrecognized output. |
| `ToolObservation.executable` | Selected native executable path, or absent when selection fails. |
| `ToolObservation.status` | Probe availability, observation, failure, or timeout description. |

Tool probes refuse scripts and symlink shims and have a three-second timeout. Builds are a separate execution path: `anchor build` for Anchor, otherwise `cargo build-sbf --manifest-path ...`. `BuildObservation` records success, the command and manifest, and the last 40 output lines. Output is shortened after capture; this is not a bound on subprocess memory or execution time.

### Evidence and report identities

`Evidence.path` is a project-relative source label; `pointer` identifies a manifest/configuration location or source line; `kind` describes provenance or a framework signal; `digest` is the SHA-256 identity of the input bytes. Evidence is a component-level list, not a complete map connecting every field to a source. Lockfile, inherited workspace, and metadata provenance need further work before claiming that stronger guarantee.

| Field | Source and behavior |
|---|---|
| `Project.name` | Root npm client name, otherwise first sorted program name, otherwise `selected project`. |
| Component collections | Sorted by component ID. Tools are empty after collection and filled only by the CLI's opt-in probes. |
| `Report.schema_version` | Current serialized report contract: 1. |
| `Report.command` | `inspect`, `check`, or `upgrade`. |
| `Report.analysis_mode` | `inventory`, `static`, or `upgrade-advice`. Appending SC100 does not change this field. |
| `Report.target` | Check/inspect selected named or explicit target ID; upgrade builds its target label from chosen framework/tool versions. |
| `Report.target_digest` | Digest for an explicit target document; absent for named targets and upgrade labels. |
| `Report.dataset.revision`, `.digest` | Validated bundled or explicitly selected dataset identity. Upgrade advice also contains knowledge in core/upgrade; the dataset digest alone does not identify that code. |
| `CheckResult.rule_id`, `.rule_revision` | Current-state metadata from the dataset; upgrade identities/revisions live in the core catalog; SC100 is assembled by the CLI. |
| `CheckResult.subject`, `.operation` | Evaluator-selected component and compatibility operation. |

`Report::finish()` sorts findings by subject and rule ID, recalculates counts, and applies exit policy. Current checks apply suppressions before finalization. Default policy fails on unsuppressed errors; strict options also fail on warnings or unknowns. Input/analysis errors are separate diagnostics with exit code 2. The semantic verdict is selected by `Report::verdict()` in core. render.rs maps that verdict to terminal wording and colors; rendering does not populate model fields. `--deny-unknown` also fails skipped checks, matching their incomplete verdict. A suppression must identify exactly one suppressible finding by rule and subject; ambiguous matches are input errors.

An opt-in build first selects exactly one program, invokes its normal build, then recollects and evaluates the resulting files even after build failure. SC100 is added to that refreshed report. Imported `--cargo-metadata` cannot accompany `--build`, because it could override new resolution with an older snapshot. Builds default to a 30-minute deadline, configurable with `--build-timeout-seconds`. Execution drains both output streams and retains at most 64 KiB per stream, then the final 40 lines for SC100. Tool probes have a three-second deadline and 8 KiB per-stream limit; unsuccessful, truncated, or ambiguous version output supplies no exact version.

## Follow one result

The mixed-workspace fixture provides a concrete SC201 example:

1. [`solcompat.toml`](../fixtures/mixed-workspace/solcompat.toml) binds the `indexer` client and its `blocks` getBlock read, selecting full JSON transactions with a maximum version of 0.
2. `collect_client()` reads package.json and package-lock.json. `collect()` adds the configured contract and asserted configuration evidence. Because reads are configured, source scanning does not add competing requests.
3. [`check()`](../crates/solcompat-core/src/rules/mod.rs) passes the client facts to the request evaluator. Reviewed request cases come from the [bundled dataset](../crates/solcompat-core/compatibility/records/rpc-read-contracts.json).
4. The evaluator compares the configured maximum with required version support and returns SC201 for the client, with the request operation, explanation, evidence, and remediation. Decoder capability is evaluated independently by SC200.
5. Suppression and exit policy are applied, then render.rs presents the result. `--detailed` expands the same result rather than running a different analysis.

Run both views to trace this yourself:

```bash
cargo run --locked -- check --path fixtures/mixed-workspace --detailed
cargo run --locked -- check --path fixtures/mixed-workspace --format json
```

For an upgrade example, UP106 activates when the resolved anchor-lang version is below 0.31, the selected target crosses 0.31, and the Rust scanner recorded `anchor-discriminator-method` evidence for `Type::discriminator()`. The scanner stores a source reference; `upgrade_report()` uses that reference to point migration advice at the affected file. It does not report this syntax migration solely because the dependency is old.

For upgrade advice, follow `analyze_upgrade()` in app.rs to `upgrade_report()` in core/upgrade/mod.rs, then to its Anchor or Agave evaluator. It uses the collected inventory but asks what changes when crossing a selected migration boundary, rather than whether the current configuration is already inconsistent.

## Construction and enrichment contracts

The program boundary is `collect_program()`: manifest declarations, framework identification, lockfile observations, and typed source signals produce the initial `Program`. `apply_program_config()` then adds asserted tool selections and configuration evidence. `apply_metadata_resolution()` replaces matching lock-derived resolution. `apply_anchor_selection()` fills only a missing Anchor selection and retains Anchor.toml evidence.

The client boundary is `collect_client()`: npm declarations and lock observations produce the initial `Client`. `apply_client_config()` validates and applies explicit contracts. When source discovery is used, `discover_rpc_reads()` supplies only missing required-version and decoder selections from supported SDK bindings.

`enrich_tool_observations()` applies observations already obtained by an opt-in probe. It performs no execution itself and cannot replace explicit selections or Anchor.toml. Collection and probing remain independently testable.

Internal `VersionResolution` values retain Cargo lock candidate sets, npm lock versions, captured Cargo metadata versions, or missing resolution. Candidate sets with multiple versions stay unresolved. `wire_versions()` is the adapter to the existing schema-1 exact-version map. This keeps the distinction between source categories and uncertainty without adding report fields. Lock and metadata resolvers enforce upstream identity before producing exact versions.

Pinocchio SC104 evaluates `cfg(test)` and `cfg(feature = "no-entrypoint")` (including `not`, `all`, and `any`) under a **package-default, non-test source profile**. It follows local feature references from `[features].default`; declaring `no-entrypoint = []` alone leaves that feature disabled. Test-only source is excluded. If defaults enable `no-entrypoint`, the excluded entrypoint cannot establish a pass. Other feature gates, target conditions, and `cfg_attr` remain unresolved when they affect reviewed source. Custom build flags and dependency feature unification are not inferred; this profile is not proof of the deployed build.

`SourceSignal` gives framework observations typed identities shared by collection and evaluation. Its schema-1 adapter preserves evidence kind strings and their ordering. Rust scans the library and its declared default-path modules. Unlinked files do not contribute signals. Unparseable files, unsupported conditional or custom-path modules, unresolved modules, and conditional items containing reviewed framework patterns carry `unresolved-rust` evidence; SC104 stays unknown rather than claiming a selected source API. Unconditional observations are retained. Conditional entrypoint imports/macros alone do not hide an unconditional signature. The account parameter is recognized by its second position and slice type, independent of its name. Component evidence remains the existing serialized list; the internal types do not create a complete per-field provenance graph.

The core rule catalog identifies each rule's evaluator, metadata owner, suppression support, and binary-owned revision where applicable. Dataset validation and configuration suppression validation share this catalog. Upgrade knowledge is still maintained in Rust, so a dataset digest alone does not identify the complete upgrade ruleset.

## Changing the implementation

Keep parsing and execution in the project crate, and current-state decisions in core. Start a source-detection change with a small fixture showing the supported construct and an adjacent case that must remain unresolved. For a refactor, compare JSON, terminal output, and exit codes before changing predicates.

The initial extraction preserved behavior. Subsequent correctness fixes retain schema 1, rule identities, and existing public entry points, while revising affected rules and the bundled dataset identity. Schema 1 adds named evidence-kind values for resolution, supported/unsupported RPC bindings, incomplete Rust scans, and framework observations already emitted previously; consumers using a strict older enum must update their validator. No required field or model shape changes. The changelog distinguishes those behavior changes from the extraction. See [CONTRIBUTING.md](../CONTRIBUTING.md) for the rule and validation workflow.
