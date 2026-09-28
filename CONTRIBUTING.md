# Contributing to SolCompat

Thanks for helping SolCompat keep up with the Solana ecosystem.

The most valuable contributions are usually small and specific: support for one documented framework migration, one client capability boundary, or one toolchain change. A narrow rule with good evidence is more useful than a broad warning that fires on every project.

## Reporting a missing compatibility change

An issue is most helpful when it includes:

- the affected framework, SDK, or tool;
- the last version that used the old behavior;
- the first version that uses the new behavior;
- a link to official release notes, documentation, a SIMD, or upstream source;
- a small before-and-after example;
- what breaks during build, deployment, RPC reading, decoding, or another concrete operation.

Please avoid reporting a version as incompatible only because it is old. SolCompat needs a specific behavior change it can explain.

## Adding or changing a rule

Start by deciding where the change belongs:

- A current-state rule (`SC...`) checks whether the source and configuration agree with the versions already resolved by the project.
- Upgrade guidance (`UP...`) explains work that becomes necessary when the user selects a newer target.
- Some release notes are useful background but do not create a project finding.

A rule should answer four questions in ordinary language:

1. What exact evidence activates it?
2. What condition passes or fails?
3. What should happen when evidence is missing?
4. What can the developer do next?

Use primary upstream sources wherever possible. Record the review date and source in the compatibility dataset. Do not assume that an unreviewed future version behaves like the newest version currently known to SolCompat.

## Source-level checks

Source checks are intentionally conservative. Match a construct only when it can be recognized without guessing what aliases, macros, wrappers, or runtime values mean.

A framework syntax migration should have tests for:

- the old version with valid old syntax;
- the new version with valid new syntax;
- old syntax paired with the new version;
- new syntax paired with the old version;
- a missing or ambiguous resolved version;
- unrelated frameworks and files.

Pinocchio SC104 is a useful example because it models two separate boundaries:

    0.9:  (&Pubkey,  &[AccountInfo])
    0.10: (&Address, &[AccountView])
    0.11: (&Address, &mut [AccountView])

## Compatibility data revisions

When a rule's compatibility meaning changes, increment both its rule revision and the dataset revision. Keep the existing rule code when the subject and purpose are unchanged. Use a new code for a different compatibility question.

Update the changelog and any documentation that describes the affected behavior. Reports include the dataset revision and digest so users can reproduce why a result changed.

## Tests and validation

Synthetic fixtures should cover the exact boundary. When practical, also run the change against a public Solana project that uses the affected release.

Before opening a pull request, run:

    cargo fmt --all -- --check
    cargo test --workspace --locked --offline
    cargo clippy --workspace --all-targets --locked --offline -- -D warnings
    python3 scripts/check-schemas.py
    scripts/release-check.sh

Please also read the compact and detailed terminal reports. A useful finding should tell a developer what was detected, why it matters, where to make the change, and how to verify it.

## Scope

SolCompat stays focused on compatibility across ecosystem changes. Security auditing, fuzzing, formal verification, validator simulation, and historical replay belong in other tools.
