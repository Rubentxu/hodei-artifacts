#!/usr/bin/env bash
# Produces the complete, locally verifiable release bundle in one run.
#
# This script only ORCHESTRATES; it reimplements nothing:
#
#   scripts/package-release.sh          tarball + .sha256
#   scripts/package-oci.sh              OCI archive, SBOM SPDX/CycloneDX,
#                                       licence matrix, provenance, manifest,
#                                       SHA256SUMS and OpenPGP signatures
#   scripts/generate-homebrew-formula.sh   Homebrew formula
#   scripts/verify-release.sh           final clean-directory verification
#
# On top of that it writes `release-bundle.json`, a signed manifest recording
# the status of every component and architecture: `ok`, or `NOT_RUN` with an
# explicit reason (for example linux/arm64 without a runner or emulator). A
# NOT_RUN entry is never reported as success. SHA256SUMS is regenerated to
# cover the formula, the release notes and the bundle manifest, and re-signed
# with the same OpenPGP key used by package-oci.sh.
#
# Usage: scripts/release-bundle.sh [output-directory]    (default: ./dist)
#
# Environment:
#   RELEASE_GPG_HOME, RELEASE_GPG_KEY_ID  operator keyring (otherwise an
#                                         ephemeral key is generated and shared
#                                         with every stage)
#   RELEASE_BUNDLE_TARBALL_CMD            overrides the tarball stage (tests)
#   RELEASE_BUNDLE_OCI_CMD                overrides the OCI stage (tests)
#
# No GitHub Actions, no network publication and no push are involved.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
BIN_NAME="hodei-artifacts-api"
BUNDLE_NAME="release-bundle.json"
FORMULA_NAME="${BIN_NAME}.rb"

TARBALL_CMD="${RELEASE_BUNDLE_TARBALL_CMD:-$ROOT/scripts/package-release.sh}"
OCI_CMD="${RELEASE_BUNDLE_OCI_CMD:-$ROOT/scripts/package-oci.sh}"

cd "$ROOT"

for tool in gpg gpgv python3 sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool is required" >&2; exit 1; }
done

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
[ -n "$VERSION" ] || { echo "error: could not read version from Cargo.toml" >&2; exit 1; }
ARCH="$(uname -m)"
NOTES_NAME="release-notes-${VERSION}.md"
ARCH_MANIFEST="${BIN_NAME}-${VERSION}-${ARCH}.manifest.json"

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
rm -f "$OUT_DIR/$BUNDLE_NAME" "$OUT_DIR/${BUNDLE_NAME}.asc" "$OUT_DIR/$FORMULA_NAME" "$OUT_DIR/$NOTES_NAME"

# --- One signing identity for every stage -----------------------------------

EPHEMERAL_GPG_DIR=""
cleanup() { [ -n "$EPHEMERAL_GPG_DIR" ] && rm -rf "$EPHEMERAL_GPG_DIR"; return 0; }
trap cleanup EXIT

if [ -n "${RELEASE_GPG_HOME:-}" ] && [ -n "${RELEASE_GPG_KEY_ID:-}" ]; then
  SIGNING_KEY_MODE="operator-keyring"
else
  EPHEMERAL_GPG_DIR="$(mktemp -d)"
  export RELEASE_GPG_HOME="$EPHEMERAL_GPG_DIR/gpghome"
  mkdir -p "$RELEASE_GPG_HOME" && chmod 700 "$RELEASE_GPG_HOME"
  cat > "$EPHEMERAL_GPG_DIR/keyparams" <<KEY
Key-Type: eddsa
Key-Curve: ed25519
Key-Usage: sign
Name-Real: Hodei Release Ephemeral Signer
Name-Email: release-ephemeral@hodei.local
Expire-Date: 0
%no-protection
%commit
KEY
  GNUPGHOME="$RELEASE_GPG_HOME" gpg --batch --gen-key "$EPHEMERAL_GPG_DIR/keyparams" >/dev/null 2>&1
  RELEASE_GPG_KEY_ID="$(GNUPGHOME="$RELEASE_GPG_HOME" gpg --with-colons --list-secret-keys | awk -F: '/^fpr:/ {print $10; exit}')"
  export RELEASE_GPG_KEY_ID
  SIGNING_KEY_MODE="ephemeral-local"
fi

# --- Stages -------------------------------------------------------------------

echo "==> [1/4] tarball"
"$TARBALL_CMD" "$OUT_DIR"

echo "==> [2/4] OCI image, SBOMs, provenance, manifest and signatures"
"$OCI_CMD" "$OUT_DIR"

if [ ! -f "$OUT_DIR/$ARCH_MANIFEST" ]; then
  echo "error: $ARCH_MANIFEST was not produced by the OCI stage" >&2
  exit 1
fi

echo "==> [3/4] Homebrew formula"
"$ROOT/scripts/generate-homebrew-formula.sh" "$OUT_DIR/$ARCH_MANIFEST" > "$OUT_DIR/$FORMULA_NAME"

# --- Honest status of the architectures this host cannot build ---------------

secondary_reason() { # <arch>
  local other="$1" qemu
  case "$other" in
    aarch64) qemu="qemu-aarch64" ;;
    x86_64) qemu="qemu-x86_64" ;;
    *) qemu="qemu-$other" ;;
  esac
  if [ ! -e "/proc/sys/fs/binfmt_misc/$qemu" ] && ! command -v "${qemu}-static" >/dev/null 2>&1; then
    echo "linux/${other}: no ${qemu} binfmt emulator and no native runner on this host (host=${ARCH}); OCI image and tarball for ${other} were not built"
  else
    echo "linux/${other}: an emulator is present but scripts/package-oci.sh builds only the host architecture (uname -m=${ARCH}); cross-architecture packaging is not wired, so it was not built"
  fi
}
SECONDARY_JSON="$(python3 -I -c '
import json, sys
print(json.dumps(dict(a.split("=", 1) for a in sys.argv[1:])))
' "x86_64=$(secondary_reason x86_64)" "aarch64=$(secondary_reason aarch64)")"

# --- Bundle manifest and release notes ----------------------------------------

echo "==> [4/4] bundle manifest, checksums and signatures"
python3 -I - "$OUT_DIR" "$BIN_NAME" "$ARCH" "$ARCH_MANIFEST" "$FORMULA_NAME" "$NOTES_NAME" \
  "$BUNDLE_NAME" "$SECONDARY_JSON" "$SIGNING_KEY_MODE" <<'PY'
import hashlib
import json
import os
import re
import sys

(out, bin_name, arch, arch_manifest, formula, notes, bundle_name,
 secondary_json, key_mode) = sys.argv[1:10]
secondary = json.loads(secondary_json)


def sha(name):
    h = hashlib.sha256()
    with open(os.path.join(out, name), "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def ref(name):
    return {"filename": name, "sha256": sha(name)}


with open(os.path.join(out, arch_manifest), encoding="utf-8") as f:
    m = json.load(f)

tar_name = m["tarball"]["filename"]
match = re.fullmatch(re.escape(f"{bin_name}-{m['version']}-") + r"(.+)\.tar\.gz", tar_name)
if not match:
    raise SystemExit(f"unexpected tarball name: {tar_name}")
native_triple = match.group(1)

APPLE = "no macOS runner or SDK on this host; Darwin tarballs need a macOS builder"
TRIPLES = [
    ("x86_64", "x86_64-unknown-linux-gnu"),
    ("aarch64", "aarch64-unknown-linux-gnu"),
    ("x86_64", "x86_64-apple-darwin"),
    ("aarch64", "aarch64-apple-darwin"),
]
components = []


def ok(component, a, files, target=None):
    c = {"component": component, "arch": a, "status": "ok", "files": files}
    if target:
        c["target"] = target
    components.append(c)


def not_run(component, a, reason, target=None):
    c = {"component": component, "arch": a, "status": "NOT_RUN", "reason": reason}
    if target:
        c["target"] = target
    components.append(c)


for a, triple in TRIPLES:
    if triple == native_triple:
        ok("tarball", a, [m["tarball"]], triple)
    elif "apple" in triple:
        not_run("tarball", a, APPLE, triple)
    else:
        not_run("tarball", a, secondary.get(a, "architecture not built on this host"), triple)

arches = sorted({arch, "x86_64", "aarch64"})
for a in arches:
    if a == arch:
        ok("oci", a, [
            {"filename": m["oci"]["archive"], "sha256": m["oci"]["sha256"]},
            ref(m["oci"]["archive"].removesuffix(".tar") + ".digest"),
        ])
        ok("sbom", a, [m["sbom"]["oci_spdx"], m["sbom"]["oci_cyclonedx"], m["sbom"]["cargo_spdx"],
                       m["license_matrix"]])
        ok("provenance", a, [m["provenance"]])
    else:
        reason = secondary.get(a, "architecture not built on this host")
        for comp in ("oci", "sbom", "provenance"):
            not_run(comp, a, reason)

ok("formula", "all", [ref(formula)])

nr = [c for c in components if c["status"] == "NOT_RUN"]
lines = [
    f"# {bin_name} {m['version']}",
    "",
    f"Commit: `{m['commit']}`",
    "",
    "## Verificación en máquina limpia",
    "",
    "```sh",
    "scripts/verify-release.sh --require-bundle <directorio-del-bundle>",
    "```",
    "",
    "Comprueba la firma OpenPGP de `SHA256SUMS`, de los manifiestos y de",
    f"`{bundle_name}`, los checksums y la correlación de SBOM, provenance y fórmula.",
    "",
    "## Componentes",
    "",
    "| Componente | Arquitectura | Estado | Detalle |",
    "| --- | --- | --- | --- |",
]
for c in components:
    detail = c.get("reason") or ", ".join(f["filename"] for f in c["files"])
    label = c["component"] + (f" ({c['target']})" if c.get("target") else "")
    lines.append(f"| {label} | {c['arch']} | {c['status']} | {detail} |")
lines += ["", "## Límites", ""]
lines += [f"- {m['signing']['honest_limits']}"]
lines += [f"- NOT_RUN: {c['component']}/{c['arch']}: {c['reason']}" for c in nr]
with open(os.path.join(out, notes), "w", encoding="utf-8") as f:
    f.write("\n".join(lines) + "\n")
ok("release_notes", "all", [ref(notes)])
ok("signing", "all", [ref(m["signing"]["public_key"])])

bundle = {
    "schema": "hodei-release-bundle/v1",
    "name": m["name"],
    "version": m["version"],
    "commit": m["commit"],
    "source_date_epoch": m["source_date_epoch"],
    "status": "partial" if nr else "complete",
    "components": components,
    "signing": {
        "mechanism": m["signing"]["mechanism"],
        "key_mode": key_mode,
        "public_key": m["signing"]["public_key"],
        "fingerprint": m["signing"]["fingerprint"],
        "signed_files": ["SHA256SUMS", bundle_name, arch_manifest],
        "honest_limits": m["signing"]["honest_limits"],
    },
}
with open(os.path.join(out, bundle_name), "w", encoding="utf-8") as f:
    json.dump(bundle, f, indent=2)
    f.write("\n")
PY

# Regenerate SHA256SUMS so it also covers the formula, notes and bundle
# manifest, then re-sign it and the bundle manifest with the shared key.
(
  cd "$OUT_DIR"
  mapfile -t LISTED < <(awk '{print $2}' SHA256SUMS)
  for extra in "$FORMULA_NAME" "$NOTES_NAME" "$BUNDLE_NAME"; do
    printf '%s\n' "${LISTED[@]}" | grep -qxF "$extra" || LISTED+=("$extra")
  done
  sha256sum "${LISTED[@]}" > SHA256SUMS.new
  mv SHA256SUMS.new SHA256SUMS
)
GNUPGHOME="$RELEASE_GPG_HOME" gpg --batch --yes --armor --detach-sign -u "$RELEASE_GPG_KEY_ID" \
  -o "$OUT_DIR/SHA256SUMS.asc" "$OUT_DIR/SHA256SUMS"
GNUPGHOME="$RELEASE_GPG_HOME" gpg --batch --yes --armor --detach-sign -u "$RELEASE_GPG_KEY_ID" \
  -o "$OUT_DIR/${BUNDLE_NAME}.asc" "$OUT_DIR/$BUNDLE_NAME"

# The bundle is only reported once it verifies from a clean directory.
echo "==> verifying the bundle from a clean directory"
"$ROOT/scripts/verify-release.sh" --require-bundle "$OUT_DIR"
echo
echo "bundle: $OUT_DIR/$BUNDLE_NAME"
