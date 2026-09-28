# What SolCompat does not prove

SolCompat answers specific compatibility questions from local evidence. A successful check means that no evaluated rule violated the chosen policy. It does not certify that an application is production-ready or compatible with every future Solana change.

## Optional checks

Some work is intentionally opt-in because it runs tools or needs a specific target:

- `--build` compiles one detected program with `anchor build` or `cargo build-sbf`.
- Artifact checks run only for an artifact supplied in `solcompat.toml`.
- sBPFv3 prerequisites apply only when `sbpf_arch = "v3"` is selected.
- `solcompat upgrade` evaluates the Anchor or Agave versions you name; it does not edit the project.
- `--probe-tools` reads installed tool versions.

## Outside the current scope

SolCompat does not deploy programs, execute transactions, inspect live cluster activation, or reproduce validator behavior. It also does not establish that a supplied `.so` came from the current source, lockfile, feature set, and toolchain.

The current rules do not cover transaction construction and signing, wallets, subscriptions, Geyser, arbitrary SDK calls, program ABI equivalence, account-layout compatibility, CPI target behavior, or whether a generated client matches a deployed program. Security auditing, fuzzing, formal verification, economic analysis, historical replay, and validator simulation are separate concerns.

## What static analysis can miss

JavaScript and TypeScript scanning recognizes direct `getBlock` and `getTransaction` calls with literal options. It may not resolve values assembled through wrappers, aliases, environment variables, helper functions, or runtime mutation. Describe those reads in `solcompat.toml` when they matter to your application.

Rust scanning recognizes reviewed source patterns inside a program crate's `src/` directory. Macros, renamed imports, aliases, generated source, wrapper types, and custom entrypoints may be ambiguous. SolCompat reports missing evidence instead of treating ambiguity as a pass.

## Version evidence

A manifest range describes allowed versions, while a lockfile or captured package metadata describes the version actually selected. SolCompat therefore uses `Cargo.lock`, `package-lock.json`, or captured Cargo metadata for checks that need exact versions.

Registry compatibility records do not automatically apply to Git dependencies, local paths, forks, files, or alternate registries. Unreviewed transaction formats, SDK releases, IDL schemas, ELF flags, and framework versions remain unknown until the dataset covers them. A dated target is a bundled knowledge snapshot, not a query against a live cluster.

## Operational limits

The default check reads local files. It does not contact RPC endpoints, download a new dataset, install tools, or execute package scripts. An opt-in build can create build outputs and may download dependencies through the underlying build tool.

Text inputs are limited to 4 MiB and artifacts to 16 MiB. Configured paths must remain inside the project root. See [Project files SolCompat understands](SUPPORTED_INPUTS.md) for discovery details.

Release validation currently covers Linux x86_64. Multi-program build selection, build matrices, artifact provenance, runtime fixtures, and captured cluster evidence are possible future additions.
