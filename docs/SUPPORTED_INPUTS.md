# Project files SolCompat understands

SolCompat combines manifests, lockfiles, source code, optional configuration, and build evidence. Missing evidence can produce `NEEDS INPUT`; a file that is unrelated to a rule is simply ignored.

| Project input | What SolCompat reads | Main limitation |
|---|---|---|
| `Cargo.toml` and Rust source | Package metadata, dependency declarations, framework identity, entrypoints, and reviewed Anchor/Pinocchio syntax | A `cdylib` crate alone does not prove it is a Solana program; macros and custom wrappers may hide syntax |
| `Cargo.lock` | Exact canonical crates.io versions linked to the program through its lockfile dependency edges | Missing owner/edge, ambiguous identity, optional dependencies, or target-specific dependencies need a suitable captured graph |
| Captured Cargo metadata | Version 1 package and resolve data passed with `--cargo-metadata` | SolCompat does not run `cargo metadata` during its normal read-only scan; imported versions conflicting with current dependency requirements remain unresolved |
| `Anchor.toml` | `toolchain.anchor_version` | Other settings are not treated as compatibility evidence unless a rule documents them |
| `package.json` and JS/TS source | Dependencies and direct `getBlock`/`getTransaction` calls with literal options | Scripts are never executed; computed options and wrappers may need configuration |
| `package-lock.json` | Exact direct npm dependency versions from lockfile formats 2 and 3 | Nested and hoisted entries are supported; Yarn, pnpm, Bun, aliases, Git/file packages, alternate registries, and unsupported version-range syntax remain unresolved |
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

Automatic discovery skips `.git`, `.anchor`, `.next`, `build`, `coverage`, `dist`, `node_modules`, and `target`. An npm package is treated as a Solana client when it declares a recognized Solana or Anchor dependency. Other packages can be bound explicitly in `solcompat.toml`. Rust program discovery follows conditionally declared default-path modules to recognize entrypoint candidates; source compatibility checks still distinguish that recognition from proof of the selected build configuration.

The current release is validated on Linux x86_64. The Rust libraries are intended to be portable, but macOS, Windows, and Linux ARM64 are not part of the claimed release test matrix yet.

## Supported source detection

For JS/TS, automatic option checks require a named `Connection` import from `@solana/web3.js`, or `createSolanaRpc` from `@solana/kit`, followed by a unique local constructor assignment. Import renames are supported. For example:

```ts
import { createSolanaRpc } from '@solana/kit';
const rpc = createSolanaRpc('https://api.mainnet-beta.solana.com');
rpc.getBlock(42n, { maxSupportedTransactionVersion: 1 });
```

Comments, quoted strings, and recognized regex literals are ignored. A simple literal options object supplies request values and documented defaults. Spreads, duplicate keys, computed keys, getters, dynamic expressions, and receiver shadowing remain unknown. A matching method on an unbound receiver produces a request-specific `NEEDS INPUT`, rather than borrowing an SDK from package.json. Mixed SDK providers need an explicit decoder contract. Nested npm packages own their own source calls.

Rust detection parses the configured library file and its declared default-path modules. It recognizes qualified Pinocchio legacy imports, selected entrypoint signatures, and reviewed Anchor call patterns with actual source locations. Unlinked files do not contribute evidence. Unresolved conditional signatures or reviewed imports, custom-path or unresolved modules, and parse failures prevent SC104 from claiming a selected source API. Conditional entrypoint imports/macros alone do not hide an unconditional signature; names such as `_accounts` and `account_views` are accepted for the second parameter. Macros and semantic type identity still need compiler validation.

Pinocchio SC104 evaluates `cfg(test)` and `cfg(feature = "no-entrypoint")` (including `not`, `all`, and `any`) under a **package-default, non-test source profile**. It follows local feature references from `[features].default`; declaring `no-entrypoint = []` alone leaves that feature disabled. Test-only source is excluded. If defaults enable `no-entrypoint`, the excluded entrypoint cannot establish a pass. Other feature gates, target conditions, and `cfg_attr` remain unresolved when they affect reviewed source. Custom build flags and dependency feature unification are not inferred; this profile is not proof of the deployed build.

Configured RPC reads take precedence for the whole client: they are not merged with scanned reads. Declare the complete relevant read contract. An empty required-version list cannot accompany configured reads or a recognized source read.
