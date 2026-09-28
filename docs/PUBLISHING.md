# Publishing SolCompat

This guide is for maintainers preparing a crates.io release and the matching GitHub archive.

## Release authentication

SolCompat uses crates.io Trusted Publishing. The release workflow proves its identity through GitHub OIDC and receives a short-lived crates.io token for that workflow run. No permanent crates.io token is stored in the repository.

Each published crate needs its own Trusted Publishing configuration:

| Field | Value |
|---|---|
| GitHub owner | `Memewtoo` |
| Repository | `solcompat` |
| Workflow filename | `release.yml` |
| Environment | empty |

The configuration must exist for `solcompat`, `solcompat-core`, and `solcompat-project`. The workflow requests `id-token: write`, authenticates with the official `rust-lang/crates-io-auth-action`, and passes its temporary token only to `cargo publish`. The action revokes the token when the job finishes.

A crate's first publication requires a regular crates.io API token because Trusted Publishing cannot be configured until the crate exists. After that first release, configure the trusted publisher, remove the GitHub secret, and revoke the bootstrap token.

## Prepare a release

Update the workspace version and move the completed changes from the `Unreleased` section of `CHANGELOG.md` into the new version. Then run the release gate from a clean checkout:

```bash
scripts/release-check.sh
```

The script checks formatting, tests, Clippy, schemas, crates.io package contents, installation from the workspace, and the Linux release archive.

When compatibility knowledge changes, also review:

- the upstream source and review date for every new boundary;
- rule and dataset revision numbers;
- fixtures on both sides of the boundary, including missing or ambiguous evidence;
- the README, rule guide, supported-input guide, limitations, and changelog; and
- compact and detailed output for a clear explanation and useful next step.

[CONTRIBUTING.md](../CONTRIBUTING.md) explains how compatibility changes are reviewed and added.

## Create a release

Push the reviewed commit to `main`, wait for CI to pass, and create an annotated tag whose version matches the workspace:

```bash
version=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "solcompat"))')

git tag -a "v$version" -m "SolCompat v$version"
git push origin main
git push origin "v$version"
```

The tag starts the release workflow. It publishes the workspace in dependency order: `solcompat-core`, `solcompat-project`, then `solcompat`. Cargo waits for each dependency to appear in the registry index before continuing. The workflow then creates a GitHub release with the Linux x86_64 archive and checksum.

A crates.io version cannot be replaced. If a release is defective, publish a corrected version. Yank the defective version only when users should stop selecting it for new dependency resolution.
