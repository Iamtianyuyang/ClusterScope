#!/usr/bin/env bash
# Build the single-file release: one `clusterscope` executable, no sibling
# server/agent binaries, no runtime frontend assets.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
[ -n "$VERSION" ] || { echo "cannot read workspace version from Cargo.toml" >&2; exit 1; }

TARGET="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUT="dist/clusterscope-${VERSION}-${TARGET}"
ARCHIVE="${OUT}.tar.gz"

cargo build --release --locked --bin clusterscope

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
install -m 0755 target/release/clusterscope "$STAGE/clusterscope"

mkdir -p dist
rm -f "$ARCHIVE" "${ARCHIVE}.sha256"
tar -C "$STAGE" -czf "$ARCHIVE" clusterscope
(cd dist && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")

echo "$ARCHIVE"
tar -tzf "$ARCHIVE"
cat "${ARCHIVE}.sha256"
