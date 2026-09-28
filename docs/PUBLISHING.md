# Publishing SolCompat

This guide is for maintainers preparing a crates.io release and the matching GitHub archive.

## One-time repository setup

Before the first release:

1. Choose the project license and add its SPDX identifier to `[workspace.package]` in the root `Cargo.toml`. Add the matching `LICENSE` file.
2. Add the final GitHub repository URL to `[workspace.package]` and replace the placeholder URL in the README.
3. Confirm that the names `solcompat`, `solcompat-core`, and `solcompat-project` are available on crates.io.
4. Add a GitHub Actions secret named `CARGO_REGISTRY_TOKEN` with permission to publish those crates.
5. Protect `main` and require the `CI / release-gate` job before merging.

Keep the crates.io token only in GitHub Actions secrets or another secret store. It should never appear in the repository, logs, or release archive.

## Prepare the release

Update the workspace version and `CHANGELOG.md`, then run the release gate from a clean checkout:

```bash
scripts/release-check.sh
```

The script checks formatting, tests, Clippy, schema files, crates.io package contents, installation from the workspace, and the Linux release archive.

When compatibility knowledge changes, also review:

- the upstream source and review date for each new boundary;
- rule and dataset revision numbers;
- fixtures on both sides of the boundary, including missing or ambiguous evidence;
- the README, rule guide, supported-input guide, limitations, and changelog; and
- compact and detailed output for a clear explanation and useful next step.

[CONTRIBUTING.md](../CONTRIBUTING.md) explains how compatibility changes are reviewed and added.

## Create the release

Push the reviewed commit to `main`, then create an annotated tag whose version matches the workspace:

```bash
version=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "solcompat"))')

git tag -a "v$version" -m "SolCompat v$version"
git push origin main
git push origin "v$version"
```

The release workflow publishes the workspace in dependency order: `solcompat-core`, `solcompat-project`, then `solcompat`. It waits for each dependency to appear in the registry index before continuing. After publication, it creates a GitHub release with the Linux x86_64 archive and checksum.

A crates.io version cannot be replaced. If a release is defective, publish a corrected version. Yank the defective version only when users should stop selecting it for new dependency resolution.
