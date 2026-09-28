# Project files SolCompat understands

SolCompat combines manifests, lockfiles, source code, optional configuration, and build evidence. Missing evidence can produce `NEEDS INPUT`; a file that is unrelated to a rule is simply ignored.

| Project input | What SolCompat reads | Main limitation |
|---|---|---|
| `Cargo.toml` and Rust source | Package metadata, dependency declarations, framework identity, entrypoints, and reviewed Anchor/Pinocchio syntax | A `cdylib` crate alone does not prove it is a Solana program; macros and custom wrappers may hide syntax |
| `Cargo.lock` | Exact crates.io package versions | Multiple versions of the same crate may need Cargo metadata to identify the direct dependency |
| Captured Cargo metadata | Version 1 package and resolve data passed with `--cargo-metadata` | SolCompat does not run `cargo metadata` during its normal read-only scan |
| `Anchor.toml` | `toolchain.anchor_version` | Other settings are not treated as compatibility evidence unless a rule documents them |
| `package.json` and JS/TS source | Dependencies and direct `getBlock`/`getTransaction` calls with literal options | Scripts are never executed; computed options and wrappers may need configuration |
| `package-lock.json` | Exact direct npm dependency versions from lockfile formats 2 and 3 | Yarn, pnpm, Bun, non-hoisted installs, Git/file packages, and alternate registries are not resolved yet |
| `solcompat.toml` | Component bindings, RPC contracts, IDLs, artifacts, targets, tool versions, and suppressions | The declarations need to describe the application's real behavior |
| Anchor IDL JSON | Legacy shape or `metadata.spec` | This does not prove ABI equivalence, deployed/source parity, or generated-client freshness |
| SBF ELF artifact | Machine, `e_flags`, sBPF generation, and digest | This does not establish source provenance or runtime success |
| Named or JSON target | A bundled local/dated policy or schema 1 target policy | Targets do not query live validator feature activation |
| Tool probes | `anchor`, `cargo-build-sbf`, and `rustc` version output | SolCompat refuses arbitrary commands and does not install missing tools |
| Opt-in build | One detected Anchor, Pinocchio, or native program | Multi-program selection and build matrices are not implemented |

## When a lockfile is missing

Many version checks need to know what Cargo actually resolved. If the project has no `Cargo.lock`, run its normal build first:

```bash
anchor build        # Anchor project
cargo build-sbf     # Pinocchio or native SBF program
```

You can also create only the lockfile with `cargo generate-lockfile`. If the workspace resolves several versions of the same crate, capture the graph and pass it explicitly:

```bash
cargo metadata --format-version 1 --locked > cargo-metadata.json
solcompat check --cargo-metadata cargo-metadata.json
```

Building is not required for every SolCompat run. It provides exact dependency resolution and, with `solcompat check --build`, direct compilation evidence.

## Discovery

Automatic discovery skips `.git`, `.anchor`, `.next`, `build`, `coverage`, `dist`, `node_modules`, and `target`. An npm package is treated as a Solana client when it declares a recognized Solana or Anchor dependency. Other packages can be bound explicitly in `solcompat.toml`.

The current release is validated on Linux x86_64. The Rust libraries are intended to be portable, but macOS, Windows, and Linux ARM64 are not part of the claimed release test matrix yet.
