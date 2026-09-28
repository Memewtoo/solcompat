# Checks and rule codes

SolCompat gives every check a stable code so that terminal output, JSON reports, and CI suppressions can refer to the same behavior. A result applies to the component and operation named in the finding. It does not make a general claim that the entire application is compatible.

The normal terminal view shows findings that need attention. Use `--detailed` to see the evidence, explanation, suggested change, and references for those findings. Checks that do not apply to a project stay out of both views.

## Program and toolchain checks

### SC001 — Rust version required by a program

When a program declares `rust-version` in `Cargo.toml`, SC001 compares it with the SBF compiler selected in `solcompat.toml`. SolCompat deliberately does not substitute the host `rustc` version because `cargo build-sbf` can use a different compiler from platform-tools.

### SC003 — Anchor CLI and `anchor-lang` alignment

SC003 compares the detected Anchor CLI release line with the resolved `anchor-lang` release line. Matching major and minor versions pass. A difference produces a warning because Cargo may have selected a compatible newer crate and the project may still build successfully.

SolCompat needs the resolved crate version from `Cargo.lock` or captured Cargo metadata. A version requirement in `Cargo.toml` is not necessarily the installed version. If no lockfile exists yet, run the project's normal build or `cargo generate-lockfile` and check again.

### SC100 — SBF program build

SC100 runs only when you pass `--build`. For a single detected program, SolCompat runs `anchor build` for Anchor projects and `cargo build-sbf --manifest-path <manifest>` for Pinocchio or native Rust programs. A successful command proves that invocation completed; it does not prove deployment or runtime behavior.

### SC102 — SBF artifact deployment eligibility

SC102 inspects an explicitly supplied SBF ELF artifact. It reads the ELF machine and `e_flags` fields to identify sBPF v0, v1, v2, or v3. Under the `future-sbpfv3-deployment` target, a v3 artifact passes while an older artifact cannot be deployed, upgraded, or finalized under that target. The local target makes no deployment-policy claim.

### SC103 — sBPFv3 build prerequisites

SC103 applies when the project explicitly selects `sbpf_arch = "v3"`. The reviewed minimums are cargo-build-sbf 4.2.0 and platform-tools 1.53.0. A Pinocchio program also needs Pinocchio 0.10.0 or newer, and a project that uses `solana-define-syscall` needs version 2.3.0 or newer. This check validates known versions; use `--build` to test the build itself.

### SC104 — Pinocchio source API alignment

SC104 compares the resolved Pinocchio version with recognizable Rust source patterns in the program's `src/` directory.

| Pinocchio line | Expected entrypoint types |
|---|---|
| 0.9.x | `&Pubkey`, `&[AccountInfo]` |
| 0.10.x | `&Address`, `&[AccountView]` |
| 0.11.x and newer reviewed releases | `&Address`, `&mut [AccountView]` |

When the syntax conflicts with the resolved version, SolCompat reports the migration needed at that boundary. Aliases, generated code, macros, and custom wrapper types may require manual review because a text scan cannot identify their meaning reliably.

## Client and RPC checks

### SC200 — Transaction decoder support

SC200 checks whether the resolved JavaScript decoder can read every transaction version that the application asks an RPC endpoint to return. It enters scope when SolCompat detects or is configured with a transaction read that accepts V1, or when a client contract explicitly requires V1.

The reviewed V1-capable releases are:

- `@solana/web3.js` 1.99.x
- `@solana/web3.js` 3.x beginning with 3.0.0-rc.3
- `@solana/kit` 8.x

An exact installed version from `package-lock.json` provides the strongest evidence. A broad dependency range alone usually cannot establish which decoder is installed.

### SC201 — RPC transaction-version acceptance

SC201 checks direct `getBlock` and `getTransaction` calls, along with explicitly configured request contracts. If a read must accept Transaction V1, its effective `maxSupportedTransactionVersion` must be at least `1`. See [Understanding SC201](SC201.md) for examples and configuration guidance.

SC200 and SC201 answer different questions: SC201 checks the RPC request option; SC200 checks the code that decodes the returned transaction.

## Anchor and IDL checks

### SC302 — `anchor-lang` and `anchor-spl` alignment

When both crates are resolved for an Anchor program, SC302 compares their major and minor release lines. A difference is a warning because compatibility may depend on the exact releases and features in use.

### SC400 — Anchor IDL reader compatibility

SC400 applies when an IDL, client, and reader package are explicitly bound in `solcompat.toml`. It distinguishes legacy Anchor IDLs from IDLs that declare `metadata.spec`, then compares that shape with the resolved reader:

- legacy IDL with pre-0.30 `@coral-xyz/anchor`
- spec IDL with 0.30–0.x `@coral-xyz/anchor`
- spec IDL with 1.x `@anchor-lang/core`

Other combinations remain unresolved unless the compatibility dataset contains evidence for them.

## Upgrade advice

`solcompat upgrade` compares the current project with versions you plan to adopt. Upgrade findings do not say the current project is broken; they describe work that becomes relevant when the selected boundary is crossed.

| Codes | Area |
|---|---|
| UP100–UP105 | Anchor release baselines, the 0.31 Agave crate transition, 0.32 build/deploy behavior, the 1.x migration, the TypeScript package rename, and 1.2 platform advice |
| UP106 | Replacement of removed `Type::discriminator()` calls with `Type::DISCRIMINATOR` and its actual length |
| UP107 | AccountInfo-style arguments to `CpiContext::new` and `new_with_signer` when moving to Anchor 1.x |
| UP200 | Reviewed program-facing and operator-facing information for the selected Agave 4.4 preview target |

The process for reviewing and adding ecosystem changes is documented in [CONTRIBUTING.md](../CONTRIBUTING.md).
