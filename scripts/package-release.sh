#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"

version="$(python3 scripts/check-maintenance.py --version)"
cargo build --release --locked

binary="$project_root/target/release/solcompat"
test "$("$binary" --version)" = "solcompat $version"

host="$(rustc -vV | awk '/^host:/ {print $2}')"
archive_root="solcompat-v${version}-${host}"
staging_parent="$(mktemp -d)"
trap 'rm -rf "$staging_parent"' EXIT
staging="$staging_parent/$archive_root"

mkdir -p "$staging/docs" "$staging/schemas" "$staging/compatibility/records" "$staging/compatibility/schema"
cp "$binary" "$staging/solcompat"
cp README.md CHANGELOG.md CONTRIBUTING.md "$staging/"
cp docs/*.md "$staging/docs/"
if [[ -f LICENSE ]]; then
  cp LICENSE "$staging/"
fi
cp schemas/report.schema.json schemas/target.schema.json "$staging/schemas/"
cp crates/solcompat-core/compatibility/records/rpc-read-contracts.json "$staging/compatibility/records/"
cp crates/solcompat-core/compatibility/schema/dataset.schema.json "$staging/compatibility/schema/"

dataset_digest="$(sha256sum crates/solcompat-core/compatibility/records/rpc-read-contracts.json | awk '{print $1}')"
dataset_revision="$(python3 -c 'import json; print(json.load(open("crates/solcompat-core/compatibility/records/rpc-read-contracts.json"))["revision"])')"
cat > "$staging/RELEASE-MANIFEST.txt" <<EOF
SolCompat version: $version
Rust host: $host
Compatibility dataset: $dataset_revision
Compatibility dataset SHA-256: $dataset_digest
Report schema: 1
Target schema: 1
Validated release host: Linux x86_64
EOF

mkdir -p dist
archive="dist/${archive_root}.tar.gz"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner -C "$staging_parent" -cf - "$archive_root" | gzip -n > "$archive"
python3 scripts/check-maintenance.py --archive "$archive"
sha256sum "$archive" > dist/SHA256SUMS

echo "$archive"
echo "dist/SHA256SUMS"
