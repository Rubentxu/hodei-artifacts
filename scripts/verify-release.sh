#!/usr/bin/env bash
# Verifies correlated release artifacts (tarball, OCI archive, digest, SBOMs,
# license matrix, provenance attestation, manifest, SHA256SUMS, and OpenPGP
# detached signatures) from a clean temporary directory so host workspace state
# cannot mask missing or tampered assets.
#
# Two layouts are understood:
#
#   * single-architecture release (scripts/package-oci.sh): one
#     `*.manifest.json` anchors the artifacts.
#   * complete bundle (scripts/release-bundle.sh): a signed `release-bundle.json`
#     records the status (ok | NOT_RUN with a reason) of every component and
#     architecture, plus the Homebrew formula and the release notes. Every
#     per-architecture manifest listed in SHA256SUMS is verified as well.
#
# Usage: scripts/verify-release.sh [--require-bundle]
#                                  [--expect-fingerprint <fpr>] [--allow-dirty-tree]
#                                  <dist-directory>
#
# `--require-bundle` fails when the directory is not a complete bundle, so a
# stripped-down single-architecture directory cannot pass as a full release.
#
# The public key, SHA256SUMS and the signatures travel together, so whoever
# tampers with the dist can replace all of them coherently and the signature
# check alone still passes: by itself it only proves integrity.
# `--expect-fingerprint <fpr>` (full fingerprint published OUTSIDE the bundle:
# release notes of another channel, a website, a signed tag...) pins the
# signer: the packaged key must have exactly that primary fingerprint,
# otherwise a release re-signed coherently with a foreign key is rejected.
# `--trusted-fingerprint` (HSP-267) is the same mechanism under its first name
# and stays as an alias; the two are not independent options.
#
# The release must have been built from a clean git tree (provenance
# `tree_clean`); `--allow-dirty-tree` accepts a knowingly dirty build and
# reports it. The licence matrix is re-checked against the licence policy
# (scripts/release-license-policy.json).

set -euo pipefail

USAGE="usage: scripts/verify-release.sh [--require-bundle] [--expect-fingerprint <fpr>] [--allow-dirty-tree] <dist-directory>"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REQUIRE_BUNDLE=0
ALLOW_DIRTY=0
TRUSTED_FPR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --require-bundle) REQUIRE_BUNDLE=1; shift ;;
    --allow-dirty-tree) ALLOW_DIRTY=1; shift ;;
    --expect-fingerprint|--trusted-fingerprint)
      [ $# -ge 2 ] || { echo "$USAGE" >&2; exit 1; }
      TRUSTED_FPR="$(printf '%s' "$2" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]')"
      if ! printf '%s' "$TRUSTED_FPR" | grep -Eq '^([0-9A-F]{40}|[0-9A-F]{64})$'; then
        echo "error: $1 must be a full OpenPGP fingerprint (40 or 64 hex digits)" >&2
        exit 1
      fi
      shift 2 ;;
    --) shift; break ;;
    -*) echo "$USAGE" >&2; exit 1 ;;
    *) break ;;
  esac
done

DIST_DIR="${1:-}"
if [ -z "$DIST_DIR" ] || [ ! -d "$DIST_DIR" ]; then
  echo "$USAGE" >&2
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
  case "$filename" in
    */*|..|.) echo "error: unsafe path in SHA256SUMS: $filename" >&2; exit 1 ;;
  esac
  if [ ! -f "$DIST_DIR/$filename" ]; then
    echo "error: listed release asset missing: $DIST_DIR/$filename" >&2
    exit 1
  fi
  cp "$DIST_DIR/$filename" "$CLEAN_DIR/$filename"
done < "$DIST_DIR/SHA256SUMS"

MANIFESTS=()
while IFS= read -r m; do MANIFESTS+=("$m"); done < <(find "$CLEAN_DIR" -maxdepth 1 -name '*.manifest.json' | sort)
if [ "${#MANIFESTS[@]}" -eq 0 ]; then
  echo "error: manifest.json missing in release bundle" >&2
  exit 1
fi

BUNDLE=""
if [ -f "$CLEAN_DIR/release-bundle.json" ]; then
  BUNDLE="$CLEAN_DIR/release-bundle.json"
elif [ "$REQUIRE_BUNDLE" -eq 1 ]; then
  echo "error: release-bundle.json is not part of the signed checksums" >&2
  exit 1
fi

# Detached signatures of every signed document must be present in the
# directory under verification (they are not listed in SHA256SUMS).
SIGNED_DOCS=("${MANIFESTS[@]}")
[ -n "$BUNDLE" ] && SIGNED_DOCS+=("$BUNDLE")
for doc in "${SIGNED_DOCS[@]}"; do
  base="$(basename "$doc")"
  if [ ! -f "$DIST_DIR/${base}.asc" ]; then
    echo "error: ${base}.asc missing in $DIST_DIR" >&2
    exit 1
  fi
  cp "$DIST_DIR/${base}.asc" "$CLEAN_DIR/${base}.asc"
done

# 0. Optional signer pinning: the packaged key must be the trusted one.
if [ -n "$TRUSTED_FPR" ]; then
  PKT_FPRS="$(gpg --batch --show-keys --with-colons "$CLEAN_DIR/release-signing-key.asc" 2>/dev/null \
    | awk -F: '/^pub:/ {want=1; next} /^fpr:/ && want {print $10; want=0}')"
  if [ "$(printf '%s\n' "$PKT_FPRS" | grep -c .)" -ne 1 ] \
    || [ "$(printf '%s' "$PKT_FPRS" | tr '[:lower:]' '[:upper:]')" != "$TRUSTED_FPR" ]; then
    echo "error: release-signing-key.asc fingerprint (${PKT_FPRS:-none}) does not match --expect-fingerprint $TRUSTED_FPR" >&2
    exit 1
  fi
fi

# 1. Verify OpenPGP signatures using gpgv against the exported public key.
gpg --dearmor < "$CLEAN_DIR/release-signing-key.asc" > "$CLEAN_DIR/trusted.gpg"
gpgv --keyring "$CLEAN_DIR/trusted.gpg" "$CLEAN_DIR/SHA256SUMS.asc" "$CLEAN_DIR/SHA256SUMS"
for doc in "${SIGNED_DOCS[@]}"; do
  gpgv --keyring "$CLEAN_DIR/trusted.gpg" "${doc}.asc" "$doc"
done

# 2. Verify SHA-256 checksums of all listed artifacts.
(
  cd "$CLEAN_DIR"
  sha256sum -c SHA256SUMS
)

# 3. Verify manifest correlation, SBOMs, license matrix, provenance and, for a
#    bundle, the status of every component.
python3 -I - "$CLEAN_DIR" "$BUNDLE" "$SCRIPT_DIR" "$ALLOW_DIRTY" "${MANIFESTS[@]}" <<'PY'
import hashlib
import json
import os
import re
import subprocess
import sys

clean_dir, bundle_path, script_dir, allow_dirty = sys.argv[1:5]
manifest_paths = sys.argv[5:]
allow_dirty = allow_dirty == "1"


def die(msg):
    raise SystemExit(msg)


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def verify_ref(ref, label):
    path = os.path.join(clean_dir, ref["filename"])
    if not os.path.isfile(path):
        die(f"{label} file missing: {ref['filename']}")
    if file_sha256(path) != ref["sha256"]:
        die(f"{label} sha256 mismatch against manifest")
    return path


def verify_arch_manifest(manifest_path):
    m = load(manifest_path)
    required_keys = (
        "name", "version", "commit", "arch", "tarball", "oci", "sbom",
        "license_matrix", "provenance", "signing",
    )
    for key in required_keys:
        if key not in m or not m[key]:
            die(f"manifest missing required field: {key}")

    verify_ref(m["tarball"], "tarball")

    oci_path = os.path.join(clean_dir, m["oci"]["archive"])
    if not os.path.isfile(oci_path):
        die(f"oci archive missing: {m['oci']['archive']}")
    if file_sha256(oci_path) != m["oci"]["sha256"]:
        die("oci archive sha256 mismatch against manifest")

    oci_spdx_path = verify_ref(m["sbom"]["oci_spdx"], "sbom.oci_spdx")
    oci_cdx_path = verify_ref(m["sbom"]["oci_cyclonedx"], "sbom.oci_cyclonedx")
    cargo_spdx_path = verify_ref(m["sbom"]["cargo_spdx"], "sbom.cargo_spdx")
    license_path = verify_ref(m["license_matrix"], "license_matrix")
    prov_path = verify_ref(m["provenance"], "provenance")

    for spdx_path in (oci_spdx_path, cargo_spdx_path):
        doc = load(spdx_path)
        if not doc.get("spdxVersion") or not doc.get("packages"):
            die(f"invalid SPDX SBOM: {spdx_path}")

    cdx = load(oci_cdx_path)
    if cdx.get("bomFormat") != "CycloneDX" or not cdx.get("components"):
        die(f"invalid CycloneDX SBOM: {oci_cdx_path}")

    lic = load(license_path)
    if lic.get("total_packages", 0) <= 0 or "surrealdb_status" not in lic:
        die("invalid license_matrix")
    # Licence policy: UNKNOWN or out-of-allowlist packages and a surrealdb/rocksdb
    # exclusion claim that disagrees with the OCI SBOM are rejected.
    policy = subprocess.run(
        [sys.executable, "-I", os.path.join(script_dir, "release-license-matrix.py"), "check",
         "--matrix", license_path, "--policy", os.path.join(script_dir, "release-license-policy.json"),
         "--oci-cdx", oci_cdx_path],
        capture_output=True, text=True)
    if policy.returncode != 0:
        die(f"licence policy check failed: {policy.stderr.strip() or policy.stdout.strip()}")
    for line in policy.stdout.splitlines():
        if line.startswith("NOT_RUN:"):
            print(f"{line} [{m['arch']}]")

    prov = load(prov_path)
    if prov.get("_type") != "https://in-toto.io/Statement/v1":
        die("invalid provenance _type")
    if prov.get("predicateType") != "https://slsa.dev/provenance/v1":
        die("invalid provenance predicateType")
    subjects = {s["name"]: s["digest"]["sha256"] for s in prov.get("subject", [])}
    if subjects.get(m["tarball"]["filename"]) != m["tarball"]["sha256"]:
        die("provenance tarball digest mismatch")
    if subjects.get(m["oci"]["archive"]) != m["oci"]["sha256"]:
        die("provenance oci archive digest mismatch")

    # Source state and architecture coherence of the provenance (HSP-289).
    params = prov.get("predicate", {}).get("buildDefinition", {}).get("externalParameters", {})
    tree_clean = params.get("tree_clean")
    if not isinstance(tree_clean, bool):
        die("provenance lacks the boolean externalParameters.tree_clean")
    if m.get("tree_clean") is not tree_clean:
        die("manifest tree_clean disagrees with the provenance")
    if not tree_clean and not allow_dirty:
        die(f"{m['arch']} was built from a dirty git tree (tree_clean=false); "
            "re-run with --allow-dirty-tree only for a knowingly unofficial build")
    if not tree_clean:
        print(f"WARNING: {m['arch']} was built from a dirty git tree (tree_clean=false)")
    if params.get("arch") != m["arch"]:
        die(f"provenance arch {params.get('arch')!r} differs from manifest arch {m['arch']!r}")
    if params.get("commit") != m["commit"]:
        die("provenance commit differs from the manifest commit")
    target_arch = str(params.get("rust_target", "")).split("-", 1)[0]
    if target_arch != m["arch"]:
        die(f"provenance rust_target {params.get('rust_target')!r} does not build {m['arch']}")
    plat = m["oci"].get("platform")
    if plat is not None and plat != {"x86_64": "linux/amd64", "aarch64": "linux/arm64"}.get(m["arch"]):
        die(f"oci platform {plat!r} does not match architecture {m['arch']!r}")
    return m


manifests = [verify_arch_manifest(p) for p in manifest_paths]

if not bundle_path:
    m = manifests[0]
    print(
        f"OK: verified {m['name']} v{m['version']} ({m['arch']}) "
        f"commit={m['commit'][:12]} oci_digest={m['oci']['digest']} "
        f"signing={m['signing']['mechanism']}/{m['signing']['key_mode']}"
    )
    raise SystemExit(0)

# ---- Bundle mode ----------------------------------------------------------

b = load(bundle_path)
if b.get("schema") != "hodei-release-bundle/v1":
    die("unsupported bundle schema")
for key in ("name", "version", "commit", "components", "status", "signing"):
    if not b.get(key):
        die(f"bundle missing required field: {key}")

with open(os.path.join(clean_dir, "SHA256SUMS"), encoding="utf-8") as f:
    listed = {line.split(None, 1)[1].strip() for line in f if line.strip()}

for m in manifests:
    if (m["name"], m["version"], m["commit"]) != (b["name"], b["version"], b["commit"]):
        die(f"manifest {m['arch']} does not belong to the bundle (name/version/commit)")

# Every per-architecture manifest must declare exactly the signing identity of
# the bundle, so a re-signed forgery cannot alter it in only one place.
for m in manifests:
    for field in ("mechanism", "key_mode", "fingerprint"):
        if m["signing"].get(field) != b["signing"].get(field):
            die(
                f"signing {field} mismatch: manifest {m['arch']} declares "
                f"{m['signing'].get(field)!r} but the bundle declares {b['signing'].get(field)!r}"
            )

STATUSES = ("ok", "NOT_RUN")
by_key = {}
for c in b["components"]:
    for key in ("component", "arch", "status"):
        if not c.get(key):
            die(f"bundle component missing field: {key}")
    label = f"{c['component']}/{c['arch']}" + (f"/{c['target']}" if c.get("target") else "")
    if c["status"] not in STATUSES:
        die(f"bundle component {label} has unknown status {c['status']!r}")
    files = c.get("files", [])
    if c["status"] == "NOT_RUN":
        if not str(c.get("reason", "")).strip():
            die(f"bundle component {label} is NOT_RUN without an explicit reason")
        if files:
            die(f"bundle component {label} is NOT_RUN but lists files")
    else:
        if not files:
            die(f"bundle component {label} is ok but lists no files")
        for ref in files:
            verify_ref(ref, label)
            if ref["filename"] not in listed:
                die(f"bundle component {label} file not covered by SHA256SUMS: {ref['filename']}")
    by_key[(c["component"], c["arch"], c.get("target", ""))] = c

has_not_run = any(c["status"] == "NOT_RUN" for c in b["components"])
if b["status"] not in ("complete", "partial"):
    die(f"unknown bundle status {b['status']!r}")
if has_not_run and b["status"] == "complete":
    die("bundle claims complete but records NOT_RUN components")
if not has_not_run and b["status"] == "partial":
    die("bundle claims partial but records no NOT_RUN components")

# Singleton components must be present and ok.
for comp in ("formula", "release_notes", "signing"):
    cs = [c for c in b["components"] if c["component"] == comp]
    if len(cs) != 1 or cs[0]["status"] != "ok":
        die(f"bundle component {comp} must be present and ok")

# At least one architecture must be fully built, and every ok OCI architecture
# must be anchored by a verified per-architecture manifest.
ok_arches = {
    c["arch"] for c in b["components"] if c["component"] == "oci" and c["status"] == "ok"
}
if not ok_arches:
    die("bundle has no architecture with a built OCI image")
for arch in ok_arches:
    for comp in ("sbom", "provenance"):
        c = by_key.get((comp, arch, ""))
        if not c or c["status"] != "ok":
            die(f"architecture {arch} has an OCI image but no ok {comp}")
    if not any(c["component"] == "tarball" and c["arch"] == arch and c["status"] == "ok"
               for c in b["components"]):
        die(f"architecture {arch} has an OCI image but no ok tarball")
if {m["arch"] for m in manifests} != ok_arches:
    die("per-architecture manifests do not match the ok architectures of the bundle")

# The Homebrew formula must pin every tarball that the bundle ships.
formula = by_key[("formula", "all", "")]["files"][0]["filename"]
with open(os.path.join(clean_dir, formula), encoding="utf-8") as f:
    ftext = f.read()
for c in b["components"]:
    if c["component"] == "tarball" and c["status"] == "ok":
        ref = c["files"][0]
        if ref["filename"] not in ftext or ref["sha256"] not in ftext:
            die(f"formula does not pin tarball {ref['filename']}")

n_ok = sum(1 for c in b["components"] if c["status"] == "ok")
n_nr = len(b["components"]) - n_ok
print(
    f"OK: verified bundle {b['name']} v{b['version']} commit={b['commit'][:12]} "
    f"status={b['status']} components_ok={n_ok} not_run={n_nr} "
    f"signing={b['signing']['mechanism']}/{b['signing']['key_mode']}"
)
for c in b["components"]:
    if c["status"] == "NOT_RUN":
        print(f"NOT_RUN: {c['component']}/{c['arch']}"
              + (f"/{c['target']}" if c.get("target") else "") + f": {c['reason']}")
PY
