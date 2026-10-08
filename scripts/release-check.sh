#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"

release_tmp="$(mktemp -d)"
trap 'rm -rf "$release_tmp"' EXIT

version="$(python3 scripts/check-maintenance.py --version)"
python3 scripts/check-maintenance.py
python3 scripts/test-maintenance.py
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked
cargo build --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
CARGO_TARGET_DIR="$release_tmp/publish" cargo publish --workspace --locked --dry-run
python3 scripts/check-maintenance.py --crate-dir "$release_tmp/publish/package"
python3 scripts/check-schemas.py
scripts/package-release.sh

cargo install --path . --locked --root target/install-smoke --force
test "$(target/install-smoke/bin/solcompat --version)" = "solcompat $version"
(cd fixtures/rpc-v1-pass && ../../target/install-smoke/bin/solcompat > /dev/null)

test "$(./target/release/solcompat --version)" = "solcompat $version"
./target/release/solcompat inspect --path fixtures/mixed-workspace --format json > /dev/null
sha256sum --check dist/SHA256SUMS

release_host="$(rustc -vV | awk '/^host:/ {print $2}')"
archive="dist/solcompat-v${version}-${release_host}.tar.gz"
python3 scripts/check-maintenance.py --archive "$archive"
extract_root="$release_tmp/extract"
mkdir -p "$extract_root"
tar -xzf "$archive" -C "$extract_root"
extracted="$extract_root/solcompat-v${version}-${release_host}/solcompat"
test "$("$extracted" --version)" = "solcompat $version"
"$extracted" inspect --path fixtures/mixed-workspace --format json > /dev/null

echo "SolCompat v${version} release checks passed."
