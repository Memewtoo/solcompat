# Changelog

## Unreleased

- Derive release versions from Cargo metadata; validate coordinated crate requirements and annotated release tags. CI artifact names and install/archive smoke tests no longer assume 0.1.0.
- Add maintenance checks for public documentation links, packaged modules/docs/schemas/licenses, and private-plan exclusions. Extend schema validation to upgrade reports and include rustdoc in the release gate.
- Document release version updates and recovery after partial crates.io publication.

- Evaluate conventional Pinocchio `no-entrypoint` and test guards under an explicit package-default, non-test source profile. Follow local default feature references, exclude test-only source, and retain uncertainty for unsupported conditions. SC104 revision 6 records this scoped source selection.

- Discover program candidates through conditionally declared default-path modules, without treating those modules as selected source. Missing or conditional sibling modules no longer hide reachable entrypoints. This restores discovery for the World Cup and Gacha Pinocchio layouts.

- Fixed Pinocchio source-detector regressions: recognize the account parameter by its position/type regardless of its name, and retain unconditional signatures beside conditional entrypoint imports/macros. Conditional framework patterns remain unresolved. Clarified that SC100 build success does not clear SC104 source-selection uncertainty.

- Resolve Cargo dependencies through the program's direct lockfile edges and canonical registry identity; retain workspace, lockfile, and captured metadata provenance. Unsupported or ambiguous dependency identities remain unresolved.
- Resolve nested npm installations for the owning client, checking upstream package identity and supported manifest requirements.
- Parse Rust syntax and declared modules so comments, string literals, and unlinked files do not become framework migration evidence. Conditional or unparseable source leaves SC104 unresolved.
- Bind direct RPC reads to supported SDK imports and constructor assignments; dynamic options, duplicate keys, spreads, shadowed or unrelated receivers remain unknown. Decoder inference follows the bound SDK rather than package priority.
- Escape terminal controls in target labels and input-error messages.
- Extend the schema-1 evidence-kind enum for resolution and source observations, including framework kinds already emitted by earlier releases. Existing field shapes remain unchanged.
- Reject contradictory no-read contracts and ambiguous suppressions. Make `--deny-unknown` fail skipped checks consistently with the report verdict.
- Bound opt-in process output and execution time. Add `--build-timeout-seconds` with a 30-minute default, reject stale imported metadata during builds, and recollect project evidence after the build.
- Revise affected rule identities' revision numbers and bundled data revision to `2026-10-08.4`; report and configuration remain schema 1. Reviewed upstream capability boundaries are unchanged.

- Added named collection/enrichment stages, typed resolution and source observations, and focused precedence/provenance tests without changing schema 1.
- Moved upgrade advice into solcompat-core, split current-state evaluators by rule family, and centralized rule ownership, suppressions, counts, exit policy, and semantic verdict selection.
- Separated CLI argument definitions and application orchestration from output handling.

- Split project collection into dedicated modules while preserving the public API, serialized reports, check outcomes, and command behavior.
- Added a contributor architecture guide covering model construction, field sources, precedence, evaluation, and tool execution.

## 0.1.0 — 2026-09-29

- Made the default scan discover direct JavaScript and TypeScript `getBlock` and `getTransaction` calls, including literal request options and source-line evidence.
- Added ancestor Cargo workspace dependency and lockfile resolution for checks started inside nested program directories.
- Made SC200 fail deterministic decoder ranges that exclude reviewed Transaction V1 support, such as `@solana/kit: ^7`.
- Added opt-in `solcompat --build` verification using `anchor build` or `cargo build-sbf` for exactly one detected program.
- Added `solcompat upgrade` with project-filtered Anchor release-boundary guidance and Agave target status.
- Reframed configuration as an optional fallback for behavior that automatic discovery cannot prove.
- Added Pinocchio source API alignment against the resolved release, including the 0.9-to-0.10 `Pubkey`/`AccountInfo` migration and the 0.10-to-0.11 mutable `AccountView` entrypoint boundary.
- Added Anchor upgrade source findings for the 0.31 discriminator constant migration and the 1.x `CpiContext` program-address migration.
- Reworked README and supporting documentation around current behavior, compatibility-data maintenance, and future framework release intake.

- Added deterministic Cargo, Anchor, Pinocchio, native-program, and npm client discovery.
- Added declared-versus-resolved dependency evidence from Cargo.lock, npm package-lock v2/v3, and supplied Cargo metadata.
- Added explicit RPC read, IDL, artifact, toolchain, and target bindings.
- Added the SC001, SC003, SC102, SC103, SC200, SC201, SC302, and SC400 compatibility rules.
- Added compact and detailed terminal output, versioned JSON, policy flags, visible suppressions, and stable exit codes.
- Added bounded SBF ELF inspection, limited Anchor IDL recognition, optional safe tool-version probes, dated targets, and conditional target files.
- Added validated embedded compatibility data and published report, dataset, and target schemas.
- Added crates.io-ready package boundaries and source installation with `cargo install --path . --locked`.
- Added `solcompat` as the compact current-directory compatibility check; `solcompat check` remains its explicit form.
- Added terminal-aware status colors, `--color auto|always|never`, `NO_COLOR` support, and a detailed status legend.
- Removed non-applicable rows from both terminal views while retaining them in JSON, and made an RPC maximum that accepts V1 activate the SC200 decoder check.
- Restricted SC201 to explicitly configured RPC reads so npm dependency discovery alone cannot create repeated unknown results.
- Made SC003 distinguish a missing Anchor CLI version from a missing resolved `anchor-lang` version and report already discovered evidence accurately.
- Documented pre-build versus post-build usage and added lockfile/build guidance to unresolved Anchor dependency diagnostics.
- Made SC003 show the manifest requirement and resolved `anchor-lang` version separately, explain Cargo-compatible ranges, and provide both alignment choices using current Anchor release guidance.
- Changed advisory-only reports to `PASSED WITH WARNINGS` and clarified that Cargo range acceptance is compatible evidence while build success still requires a controlled build receipt.
- Accepted current `EM_SBPF` (263) artifacts alongside legacy `EM_BPF` (247), and explained how Pinocchio checks become applicable when an otherwise valid run has no selected compatibility scenario.

Live deployment, validator simulation, live-cluster inspection, ABI equivalence, and universal compatibility claims remain outside the current scope.
