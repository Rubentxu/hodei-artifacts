#!/usr/bin/env bash
# Verifies correlated release artifacts (tarball, OCI archive, digest, SBOMs,
# license matrix, provenance attestation, manifest, SHA256SUMS, and OpenPGP
# detached signatures) from a clean temporary directory so host workspace state
# cannot mask missing or tampered assets.
#
# Usage: scripts/verify-release.sh <dist-directory>

set -euo pipefail

DIST_DIR="${1:-}"
if [ -z "$DIST_DIR" ] || [ ! -d "$DIST_DIR" ]; then
  echo "usage: scripts/verify-release.sh <dist-directory>" >&2
  exit 1
fi

for req in SHA256SUMS SHA256SUMS.asc release-signing-key.asc; do
  if [ ! -f "$DIST_DIR/$req" ]; then
    echo "error: $req missing in $DIST_DIR" >&2
    exit 1
  fi
done

CLEAN_DIR="$(mktemp -d)"
trap 'rm -rf "$CLEAN_DIR"' EXIT

# Copy only the files named in SHA256SUMS plus SHA256SUMS and detached
# signatures into CLEAN_DIR.
cp "$DIST_DIR/SHA256SUMS" "$CLEAN_DIR/SHA256SUMS"
cp "$DIST_DIR/SHA256SUMS.asc" "$CLEAN_DIR/SHA256SUMS.asc"
cp "$DIST_DIR/release-signing-key.asc" "$CLEAN_DIR/release-signing-key.asc"

while read -r _hash filename; do
  if [ ! -f "$DIST_DIR/$filename" ]; then
    echo "error: listed release asset missing: $DIST_DIR/$filename" >&2
    exit 1
  fi
  cp "$DIST_DIR/$filename" "$CLEAN_DIR/$filename"
done < "$DIST_DIR/SHA256SUMS"

MANIFEST="$(find "$CLEAN_DIR" -maxdepth 1 -name '*.manifest.json' | head -n 1)"
if [ -z "$MANIFEST" ]; then
  echo "error: manifest.json missing in release bundle" >&2
  exit 1
fi
MANIFEST_BASE="$(basename "$MANIFEST")"
if [ ! -f "$DIST_DIR/${MANIFEST_BASE}.asc" ]; then
  echo "error: ${MANIFEST_BASE}.asc missing in $DIST_DIR" >&2
  exit 1
fi
cp "$DIST_DIR/${MANIFEST_BASE}.asc" "$CLEAN_DIR/${MANIFEST_BASE}.asc"

# 1. Verify OpenPGP signatures using gpgv against the exported public key.
gpg --dearmor < "$CLEAN_DIR/release-signing-key.asc" > "$CLEAN_DIR/trusted.gpg"
gpgv --keyring "$CLEAN_DIR/trusted.gpg" "$CLEAN_DIR/SHA256SUMS.asc" "$CLEAN_DIR/SHA256SUMS"
gpgv --keyring "$CLEAN_DIR/trusted.gpg" "$CLEAN_DIR/${MANIFEST_BASE}.asc" "$CLEAN_DIR/$MANIFEST_BASE"

# 2. Verify SHA-256 checksums of all listed artifacts.
(
  cd "$CLEAN_DIR"
  sha256sum -c SHA256SUMS
)

# 3. Verify manifest correlation, SBOMs, license matrix, and provenance.
python3 - "$CLEAN_DIR" "$MANIFEST" <<'PY'
import hashlib
import json
import os
import sys

clean_dir, manifest_path = sys.argv[1], sys.argv[2]
with open(manifest_path, "r", encoding="utf-8") as f:
    m = json.load(f)

required_keys = (
    "name",
    "version",
    "commit",
    "arch",
    "tarball",
    "oci",
    "sbom",
    "license_matrix",
    "provenance",
    "signing",
)
for key in required_keys:
    if key not in m or not m[key]:
        raise SystemExit(f"manifest missing required field: {key}")

def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def verify_ref(ref, label):
    path = os.path.join(clean_dir, ref["filename"])
    if not os.path.isfile(path):
        raise SystemExit(f"{label} file missing: {ref['filename']}")
    actual = file_sha256(path)
    if actual != ref["sha256"]:
        raise SystemExit(f"{label} sha256 mismatch against manifest")
    return path

tarball_path = verify_ref(m["tarball"], "tarball")

oci_path = os.path.join(clean_dir, m["oci"]["archive"])
if file_sha256(oci_path) != m["oci"]["sha256"]:
    raise SystemExit("oci archive sha256 mismatch against manifest")

oci_spdx_path = verify_ref(m["sbom"]["oci_spdx"], "sbom.oci_spdx")
oci_cdx_path = verify_ref(m["sbom"]["oci_cyclonedx"], "sbom.oci_cyclonedx")
cargo_spdx_path = verify_ref(m["sbom"]["cargo_spdx"], "sbom.cargo_spdx")
license_path = verify_ref(m["license_matrix"], "license_matrix")
prov_path = verify_ref(m["provenance"], "provenance")

# Validate SPDX & CycloneDX SBOM structure.
for spdx_path in (oci_spdx_path, cargo_spdx_path):
    with open(spdx_path, "r", encoding="utf-8") as f:
        doc = json.load(f)
    if not doc.get("spdxVersion") or not doc.get("packages"):
        raise SystemExit(f"invalid SPDX SBOM: {spdx_path}")

with open(oci_cdx_path, "r", encoding="utf-8") as f:
    cdx = json.load(f)
if cdx.get("bomFormat") != "CycloneDX" or not cdx.get("components"):
    raise SystemExit(f"invalid CycloneDX SBOM: {oci_cdx_path}")

# Validate license matrix.
with open(license_path, "r", encoding="utf-8") as f:
    lic = json.load(f)
if lic.get("total_packages", 0) <= 0 or "surrealdb_status" not in lic:
    raise SystemExit("invalid license_matrix")

# Validate provenance attestation subjects match tarball and OCI archive.
with open(prov_path, "r", encoding="utf-8") as f:
    prov = json.load(f)
if prov.get("_type") != "https://in-toto.io/Statement/v1":
    raise SystemExit("invalid provenance _type")
if prov.get("predicateType") != "https://slsa.dev/provenance/v1":
    raise SystemExit("invalid provenance predicateType")

subjects = {s["name"]: s["digest"]["sha256"] for s in prov.get("subject", [])}
if subjects.get(m["tarball"]["filename"]) != m["tarball"]["sha256"]:
    raise SystemExit("provenance tarball digest mismatch")
if subjects.get(m["oci"]["archive"]) != m["oci"]["sha256"]:
    raise SystemExit("provenance oci archive digest mismatch")

print(
    f"OK: verified {m['name']} v{m['version']} ({m['arch']}) "
    f"commit={m['commit'][:12]} oci_digest={m['oci']['digest']} "
    f"signing={m['signing']['mechanism']}/{m['signing']['key_mode']}"
)
PY
