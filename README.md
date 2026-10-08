# 🔄 SolCompat — Solana Compatibility Checker

[![Crates.io](https://img.shields.io/crates/v/solcompat.svg)](https://crates.io/crates/solcompat)
[![Downloads](https://img.shields.io/crates/d/solcompat.svg)](https://crates.io/crates/solcompat)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/Memewtoo/solcompat/actions/workflows/ci.yml/badge.svg)](https://github.com/Memewtoo/solcompat/actions/workflows/ci.yml)

SolCompat helps answer a question that comes up whenever Solana ships a major change:

**Will this project still work after we update its framework, toolchain, or client libraries?**

Run it from a Solana project directory and it will inspect the programs and clients it recognizes. The default report is a short checklist. When something needs attention, the detailed report explains what SolCompat found, where it found it, and what to change.

SolCompat works offline during normal checks. It reads project files but does not modify them.

## Install

Install the published crate:

    cargo install solcompat --locked

Or install a local checkout:

    git clone https://github.com/Memewtoo/solcompat.git
    cd solcompat
    cargo install --path . --locked

Cargo usually places the executable in `$HOME/.cargo/bin`. Check that it is available:

    solcompat --version

## Check a project

Change into the project and run:

    solcompat

For more context:

    solcompat --detailed

A compact report is meant for a quick check during development. Detailed mode includes evidence, file locations, suggested changes, and links to the upstream documentation used by the rule.

Other useful commands:

| Command | What it does |
|---|---|
| `solcompat inspect` | Shows the programs, clients, versions, IDLs, artifacts, and RPC reads that were discovered |
| `solcompat check` | Runs the same compatibility check as the command without a subcommand |
| `solcompat --build` | Builds one detected program and includes the result in the report |
| `solcompat upgrade` | Explains changes involved in moving to selected Anchor and Agave releases |
| `solcompat check --format json` | Produces a report for CI or another tool |
| `solcompat check --path ../project` | Checks a project outside the current directory |

The `--detailed` flag changes only the terminal display. It does not run additional checks, and it does not change JSON output.

## What SolCompat currently checks

SolCompat has a deliberately small set of checks. Each one covers a specific compatibility question for which the project can provide reliable local evidence.

### Programs and frameworks

- Whether a selected SBF compiler satisfies a package's Rust version requirement.
- Whether the Anchor CLI and resolved `anchor-lang` crate are on the same release line.
- Whether `anchor-lang` and `anchor-spl` are aligned within a program.
- Whether an explicitly selected sBPFv3 build profile has the required tools and framework versions.
- Whether a supplied SBF artifact is accepted by an explicitly selected deployment policy.
- Whether Pinocchio source uses the API expected by its resolved version.

Pinocchio has two source-level migrations covered today:

| Pinocchio release | Entrypoint types |
|---|---|
| 0.9.x | `&Pubkey` and `&[AccountInfo]` |
| 0.10.x | `&Address` and `&[AccountView]` |
| 0.11.x | `&Address` and `&mut [AccountView]` |

A program on 0.9 receives guidance for both migration steps. If the source already uses an API that its resolved Pinocchio version cannot provide, SolCompat reports an error.

Pinocchio checks understand the conventional `no-entrypoint` gate using package default features and exclude test-only code. Declaring the feature alone does not disable the entrypoint. Custom feature flags are not inferred; the detailed result states the source profile used.

### Transaction V1 clients

SolCompat finds direct JavaScript and TypeScript calls to `getBlock` and `getTransaction` when their options are written as literals. The automatic path binds supported SDK imports and local constructor assignments. Wrappers, unbound receivers, and dynamic options may need configuration. It checks both sides of Transaction V1 support:

1. Does the RPC request accept V1 through `maxSupportedTransactionVersion`?
2. Can the resolved client package decode V1 responses?

For example:

    import { createSolanaRpc } from "@solana/kit";
    const rpc = createSolanaRpc("https://api.mainnet-beta.solana.com");
    const block = await rpc.getBlock(slot, {
      encoding: "json",
      transactionDetails: "full",
      maxSupportedTransactionVersion: 1,
    });

The currently reviewed decoder releases are:

- `@solana/web3.js` 1.99.x
- `@solana/web3.js` 3.x from 3.0.0-rc.3
- `@solana/kit` 8.x

`maxSupportedTransactionVersion` belongs in the actual RPC request. Adding it only to SolCompat configuration does not change application behavior.

### Upgrade guidance

`solcompat upgrade` compares the project's resolved versions with selected targets:

    solcompat upgrade --anchor 1.2.0 --agave 4.3.0 --detailed

Anchor guidance currently covers the 0.31, 0.32, 1.x, and 1.2 release boundaries. Where possible, SolCompat narrows the advice using source evidence. For example, it can point out a removed `Type::discriminator()` call or an old `CpiContext::new` program argument.

Agave guidance distinguishes changes that affect application programs from changes aimed at validator and RPC operators. A validator release note is not automatically presented as a program failure.

## Builds and lockfiles

You can run SolCompat before building. If `Cargo.lock` or `package-lock.json` already exists, it can use the resolved versions immediately.

If Cargo resolution is the only missing information:

    cargo generate-lockfile
    solcompat

You can also build normally first:

    anchor build       # Anchor
    cargo build-sbf    # Pinocchio or native
    solcompat

Or let SolCompat run a single detected program build:

    solcompat --build

Builds are opt-in because the underlying tools can create files and download dependencies. SolCompat checks the resulting files again after the build, including any new lockfile. The default deadline is 30 minutes; for example, `solcompat --build --build-timeout-seconds 600` allows ten minutes.

If lockfile dependency edges cannot identify the program’s dependency, capture the resolved graph:

    cargo metadata --format-version 1 --locked > solcompat-metadata.json
    solcompat --cargo-metadata solcompat-metadata.json

Keep the captured file inside the project selected by `--path`. Run this as a separate check after building; `--cargo-metadata` and `--build` cannot be combined.

## Understanding the report

- **PASS** means the named check was verified with the evidence shown.
- **ERROR** means SolCompat found a reviewed incompatibility.
- **WARNING** means the current project may still work, but an upgrade or version alignment deserves attention.
- **NEEDS INPUT** means the check applies but an exact version or behavior could not be established.

Rules that do not apply are left out of terminal output. They remain in JSON so automated consumers can see the complete evaluation.

Exit code 0 means no result violated the selected policy. Exit code 1 means an error was found, or a warning/unknown was denied by a policy flag. Exit code 2 means the input was invalid or a requested operation could not start.

A successful report covers only the checks that ran. It is not a promise that the whole application will behave correctly in production.

## Configuration

Most projects do not need `solcompat.toml`. Add one when behavior cannot be seen directly, such as an RPC wrapper that assembles options dynamically.

    schema_version = 1

    [[clients]]
    id = "indexer"
    manifest = "package.json"
    decoder_package = "@solana/web3.js"
    required_read_versions = ["legacy", "v0", "v1"]

    [[clients.rpc_reads]]
    id = "blocks"
    method = "getBlock"
    encoding = "json"
    transaction_details = "full"
    max_supported_transaction_version = 1

Configuration describes the application; it does not alter it.

## Project scope

SolCompat is a compatibility assistant rather than a security scanner or runtime simulator. It does not audit authorization, fuzz programs, replay chain history, inspect live clusters, prove ABI equivalence, or predict application-specific Alpenglow behavior.

For precise coverage and caveats, see:

- [Rules and checks](docs/RULES.md)
- [Supported project inputs](docs/SUPPORTED_INPUTS.md)
- [Known limitations](docs/LIMITATIONS.md)
- [Transaction-version checking](docs/SC201.md)
- [Using SolCompat in CI](docs/CI.md)

Contributions that add support for new Solana, Agave, Anchor, Pinocchio, or client releases are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains how compatibility claims are reviewed.

## Development

    cargo fmt --all -- --check
    cargo test --workspace --locked --offline
    cargo clippy --workspace --all-targets --locked --offline -- -D warnings
    python3 scripts/check-schemas.py
    python3 scripts/check-maintenance.py
    python3 scripts/test-maintenance.py
    scripts/release-check.sh

Read [the architecture guide](docs/ARCHITECTURE.md) to follow model construction, field sources, rule evaluation, and reporting.

The workspace contains three packages:

- `solcompat` provides the CLI, application orchestration, and report rendering.
- `solcompat-core` contains the report model, compatibility data, current-state rules, and upgrade advice.
- `solcompat-project` discovers projects and collects local evidence.

## License

SolCompat is licensed under the [Apache License 2.0](LICENSE).
