# Running SolCompat in CI

Pin the SolCompat release in CI so that a new publication does not change results without review. JSON output is useful as a build artifact because it records the project evidence, selected target, dataset revision, and dataset digest used for the decision.

## Install from crates.io

When the job already has Rust installed:

```yaml
- name: Install SolCompat
  run: cargo install solcompat --version 0.1.0 --locked

- name: Check Solana compatibility
  run: solcompat check --path . --format json > solcompat-report.json
```

Upload `solcompat-report.json` even when the check fails so that the full evidence remains available to reviewers.

## Use a release archive

A repository can instead download or cache the matching GitHub release archive. Verify the archive against `SHA256SUMS`, unpack it, and invoke the pinned binary:

```yaml
name: solcompat

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  compatibility:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v7

      - name: Run SolCompat
        run: |
          ./tools/solcompat-v0.1.0-x86_64-unknown-linux-gnu/solcompat \
            check --path . --format json > solcompat-report.json

      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: solcompat-report
          path: solcompat-report.json
```

The example assumes the verified archive has been restored into `tools/`. Downloading it during the job is also reasonable when the workflow checks the published checksum.

## Choose a policy gradually

A default check fails on incompatibilities and succeeds when it finds only warnings or missing evidence. This is a practical starting point for an existing project.

Add `--deny-unknown` after the repository has supplied the evidence needed by its applicable rules. Add `--deny-warnings` only when advisory migration and version-alignment findings should block a merge. Keep conditional or dated targets explicit rather than allowing a CI environment to choose them implicitly.

Planned upgrades work well as a separate job:

```bash
solcompat upgrade \
  --anchor 1.2.0 \
  --agave 4.3.0 \
  --format json > solcompat-upgrade.json
```

Review a SolCompat dependency update in the same way as a compiler or framework update: read the changelog, note the new dataset revision, and inspect newly applicable findings before merging it.
