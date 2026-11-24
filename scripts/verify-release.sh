#!/usr/bin/env bash
# Verifies correlated release artifacts (tarball, OCI archive, digest, manifest,
# and SHA256SUMS) from a clean temporary directory so host workspace state
# cannot mask missing or tampered assets.
#
# Usage: scripts/verify-release.sh <dist-directory>

set -euo pipefail

DIST_DIR="${1:-}"
if [ -z "$DIST_DIR" ] || [ ! -d "$DIST_DIR" ]; then
  echo "usage: scripts/verify-release.sh <dist-directory>" >&2
  exit 1
fi

if [ ! -f "$DIST_DIR/SHA256SUMS" ]; then
  echo "error: SHA256SUMS missing in $DIST_DIR" >&2
  exit 1
fi

CLEAN_DIR="$(mktemp -d)"
trap 'rm -rf "$CLEAN_DIR"' EXIT

# Copy only the files named in SHA256SUMS + SHA256SUMS itself into CLEAN_DIR.
cp "$DIST_DIR/SHA256SUMS" "$CLEAN_DIR/SHA256SUMS"
while read -r _hash filename; do
  if [ ! -f "$DIST_DIR/$filename" ]; then
    echo "error: listed release asset missing: $DIST_DIR/$filename" >&2
    exit 1
  fi
  cp "$DIST_DIR/$filename" "$CLEAN_DIR/$filename"
done < "$DIST_DIR/SHA256SUMS"

(
  cd "$CLEAN_DIR"
  sha256sum -c SHA256SUMS
)

MANIFEST="$(find "$CLEAN_DIR" -maxdepth 1 -name '*.manifest.json' | head -n 1)"
if [ -z "$MANIFEST" ]; then
  echo "error: manifest.json missing in release bundle" >&2
  exit 1
fi

python3 - "$CLEAN_DIR" "$MANIFEST" <<'PY'
import hashlib
import json
import os
import sys

clean_dir, manifest_path = sys.argv[1], sys.argv[2]
with open(manifest_path, "r", encoding="utf-8") as f:
    m = json.load(f)

for key in ("name", "version", "commit", "arch", "tarball", "oci"):
    if key not in m or not m[key]:
        raise SystemExit(f"manifest missing required field: {key}")

def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

tarball_path = os.path.join(clean_dir, m["tarball"]["filename"])
oci_path = os.path.join(clean_dir, m["oci"]["archive"])

if file_sha256(tarball_path) != m["tarball"]["sha256"]:
    raise SystemExit("tarball sha256 mismatch against manifest")
if file_sha256(oci_path) != m["oci"]["sha256"]:
    raise SystemExit("oci archive sha256 mismatch against manifest")

print(
    f"OK: verified {m['name']} v{m['version']} ({m['arch']}) "
    f"commit={m['commit'][:12]} oci_digest={m['oci']['digest']}"
)
PY
