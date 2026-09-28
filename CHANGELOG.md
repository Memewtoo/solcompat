# Changelog

## Unreleased

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
