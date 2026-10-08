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

Update `[workspace.package].version` in the root `Cargo.toml` and the version requirements for `solcompat-core` and `solcompat-project` in both root and project-crate manifests. All three crates share one version. Refresh `Cargo.lock` with `cargo check --workspace`, then run `python3 scripts/check-maintenance.py`. Move the completed changes from the `Unreleased` section of `CHANGELOG.md` into the new version. Then run the release gate from a clean checkout:

```bash
scripts/release-check.sh
```

The script checks formatting, tests, Clippy, rustdoc, catalog/dataset consistency, report schemas, local documentation links, crates.io publish dry-run verification, crate contents, installation from the workspace, and archive extraction. It needs registry access and a clean checkout. Uncommitted files are intentionally rejected by Cargo packaging. It never uploads crates. Archive names, binary smoke checks, and CI artifact names use the validated Cargo workspace version.

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
version=$(python3 scripts/check-maintenance.py --version)

git tag -a "v$version" -m "SolCompat v$version"
git push origin main
git push origin "v$version"
```

The tag starts the release workflow. It publishes the workspace in dependency order: `solcompat-core`, `solcompat-project`, then `solcompat`. Cargo waits for each dependency to appear in the registry index before continuing. The workflow then creates a GitHub release with the Linux x86_64 archive and checksum.

A crates.io version cannot be replaced. If a release is defective, publish a corrected version. Yank the defective version only when users should stop selecting it for new dependency resolution.


## Recover from partial publication

A failed workflow may already have uploaded one or two crates. Do not bump the version just to retry a failed step, recreate a completed release, or assume a registry-index timeout means the upload failed. Cargo documents that an upload can succeed even when waiting for the index times out.

1. Read the failed workflow log to find the first unfinished step. Check the intended version on each crates.io version page: [core](https://crates.io/crates/solcompat-core/versions), [project](https://crates.io/crates/solcompat-project/versions), and [CLI](https://crates.io/crates/solcompat/versions). If a version exists but is not yet in the index, wait for indexing before retrying.
2. Continue from the same reviewed tagged commit. Verify that already published packages match that commit and intended contents; use the registry's crate download/checksum and the original workflow artifacts when needed. Published crate contents cannot be replaced. A content mismatch requires a corrected release version.
3. Open the Release workflow in GitHub Actions and choose **Run workflow** on `main`. Set `release_tag` to the original annotated tag. Select `published_packages`: `core` if only core exists, `core-and-project` if those two exist, or `all` if every crate exists. The recovery run checks out and validates the tagged commit, runs its full release gate, and publishes only remaining packages. `none` performs the usual full publication. Confirm the registry state before choosing; this is a maintainer assertion, not automatic duplicate detection.
4. If all crates exist and only GitHub release creation failed, select `all` to skip registry authentication and publication. Check that the regenerated archive matches the original verified artifacts. If the GitHub release already exists, inspect its assets and repair the missing assets separately; the workflow does not overwrite an existing release or recreate its tag.

The workflow definition used for manual recovery must already be on `main`; the checked-out source comes from `release_tag`. Keep the original commit and tag unchanged. Trusted Publishing continues to use `release.yml` and short-lived workflow credentials. The original tag-triggered path still publishes all three packages.

See Cargo's [publish documentation](https://doc.rust-lang.org/cargo/commands/cargo-publish.html) for upload/index behavior and [package documentation](https://doc.rust-lang.org/cargo/commands/cargo-package.html) for package verification.
