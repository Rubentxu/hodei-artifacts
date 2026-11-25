#!/usr/bin/env bash
# Packages the LEGACY tool surface: the operator binaries that read and
# migrate the embedded engine, isolated from the production distribution.
#
# HSP-213 retired SurrealDB and RocksDB from the product binary. The tool
# that still has to look at a local store during the migration window is
# NOT shipped inside the service package: it is built here, with the
# `legacy` feature explicitly on, from the same source tree, and published
# as its own artifact. The migration window opened on 2026-11-22 and
# closes only after the performance spine and the retirement of the reader
# are done; when it closes, this script and the `legacy` feature go away
# together — the export of the original store is the evidence that stays.
#
# What it produces:
#
#   <out>/hodei-legacy-tools-<version>-<target>.tar.gz
#   <out>/hodei-legacy-tools-<version>-<target>.tar.gz.sha256
#
# Usage: scripts/package-legacy-tools.sh [output-directory]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"

cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
if [ -z "$VERSION" ]; then
  echo "error: could not read the version from Cargo.toml" >&2
  exit 1
fi

TARGET="$(rustc -vV | sed -n 's/^host: //p')"
if [ -z "$TARGET" ]; then
  echo "error: could not read the host target from rustc" >&2
  exit 1
fi

TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "$TARGET_DIR" ]; then
  echo "error: could not read the cargo target directory" >&2
  exit 1
fi

# The tool surface: every binary that needs the embedded engine. Each one
# is declared with required-features = ["legacy"] in the manifest, so this
# is also the proof that the cut is real: a binary that forgot its gate
# would fail to build under --no-default-features, and the closure check
# below would name it.
BINS=(hodei-recovery ensure-iam-index ensure-graph-index backfill-iam-scope)

echo "==> building the legacy tool surface (--features legacy)"
cargo build --release --features legacy --bin "${BINS[@]}"

PKG_NAME="hodei-legacy-tools-${VERSION}-${TARGET}"
STAGE="$OUT_DIR/stage/$PKG_NAME"
ARCHIVE="$OUT_DIR/${PKG_NAME}.tar.gz"

echo "==> assembling $PKG_NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin"

for bin in "${BINS[@]}"; do
  path="$(find "${TARGET_DIR}/release" -maxdepth 1 -type f -name "$bin" -perm -u+x | head -n 1)"
  if [ -z "$path" ]; then
    echo "error: the legacy tool $bin was not produced" >&2
    exit 1
  fi
  install -m 0755 "$path" "$STAGE/bin/$bin"
done

printf '%s\n' \
  "HSP-213 legacy tool surface." \
  "" \
  "These binaries read and migrate the embedded engine (SurrealDB over" \
  "RocksDB). They are NOT part of the production distribution, and they" \
  "are shipped here only while the migration window is open (window" \
  "opened 2026-11-22). The product service boots on the external graph" \
  "authority alone; a deployment that still points at a local store is" \
  "refused at boot with the migration message." \
  > "$STAGE/README.md"

echo "==> archiving"
mkdir -p "$OUT_DIR"
rm -f "$ARCHIVE" "$ARCHIVE.sha256"

SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}"

tar \
  --sort=name \
  --mtime="@${SOURCE_DATE_EPOCH}" \
  --owner=0 --group=0 --numeric-owner \
  --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
  -C "$OUT_DIR/stage" -czf "$ARCHIVE" "$PKG_NAME"

( cd "$OUT_DIR" && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256" )

# The same promise from the other side: the tools prove that WITHOUT the
# feature the engine is really gone from the tree of the service.
"$ROOT/scripts/check-production-closure.sh"

echo
echo "legacy tool distribution: $ARCHIVE"
echo "sha256:                  $(cut -d' ' -f1 < "$ARCHIVE.sha256")"
