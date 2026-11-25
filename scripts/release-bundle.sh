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
#   RELEASE_BUNDLE_EXTRA_ARCHES           space-separated architectures built in
#                                         addition to the host one (HSP-287),
#                                         for example "aarch64": each one is
#                                         packaged by the OCI stage with
#                                         HODEI_OCI_ARCH, and the bundle gains a
#                                         correlated OCI image index (manifest
#                                         list) that pins every per-arch image
#                                         manifest digest. Without it, the other
#                                         architectures are recorded as NOT_RUN.
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
rm -f "$OUT_DIR/$BUNDLE_NAME" "$OUT_DIR/${BUNDLE_NAME}.asc" "$OUT_DIR/$FORMULA_NAME" "$OUT_DIR/$NOTES_NAME" "$OUT_DIR/${BIN_NAME}-${VERSION}-oci-index.json"

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
# Tell the stages the true origin of the shared key (they only see RELEASE_GPG_*).
export RELEASE_SIGNING_KEY_MODE="$SIGNING_KEY_MODE"

# --- Stages -------------------------------------------------------------------

echo "==> [1/4] tarball"
"$TARBALL_CMD" "$OUT_DIR"

echo "==> [2/4] OCI image, SBOMs, provenance, manifest and signatures"
"$OCI_CMD" "$OUT_DIR"

EXTRA_ARCHES="${RELEASE_BUNDLE_EXTRA_ARCHES:-}"
EXTRA_MANIFESTS=()
cp "$OUT_DIR/SHA256SUMS" "$OUT_DIR/SHA256SUMS.host"
for extra_arch in $EXTRA_ARCHES; do
  [ "$extra_arch" != "$ARCH" ] || { echo "error: extra architecture $extra_arch is the host one" >&2; exit 1; }
  echo "==> [2b/4] OCI image and tarball for $extra_arch"
  HODEI_OCI_ARCH="$extra_arch" "$OCI_CMD" "$OUT_DIR"
  EXTRA_MANIFESTS+=("${BIN_NAME}-${VERSION}-${extra_arch}.manifest.json")
  # Each OCI stage rewrites SHA256SUMS for its own architecture; keep the union.
  cat "$OUT_DIR/SHA256SUMS" >> "$OUT_DIR/SHA256SUMS.host"
done
if [ -n "$EXTRA_ARCHES" ]; then
  sort -u -k2 "$OUT_DIR/SHA256SUMS.host" > "$OUT_DIR/SHA256SUMS"
fi
rm -f "$OUT_DIR/SHA256SUMS.host"

if [ ! -f "$OUT_DIR/$ARCH_MANIFEST" ]; then
  echo "error: $ARCH_MANIFEST was not produced by the OCI stage" >&2
  exit 1
fi

echo "==> [3/4] Homebrew formula"
FORMULA_ARGS=("$OUT_DIR/$ARCH_MANIFEST")
for em in ${EXTRA_MANIFESTS[@]+"${EXTRA_MANIFESTS[@]}"}; do FORMULA_ARGS+=("$OUT_DIR/$em"); done
"$ROOT/scripts/generate-homebrew-formula.sh" "${FORMULA_ARGS[@]}" > "$OUT_DIR/$FORMULA_NAME"

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
  "$BUNDLE_NAME" "$SECONDARY_JSON" "$SIGNING_KEY_MODE" "$EXTRA_ARCHES" <<'PY'
import hashlib
import json
import os
import re
import sys

(out, bin_name, arch, arch_manifest, formula, notes, bundle_name,
 secondary_json, key_mode, extra_arches) = sys.argv[1:11]
secondary = json.loads(secondary_json)
extra_arches = extra_arches.split()


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

# Every architecture that was really built, anchored by its own manifest.
manifests = {arch: m}
for a in extra_arches:
    with open(os.path.join(out, f"{bin_name}-{m['version']}-{a}.manifest.json"), encoding="utf-8") as f:
        manifests[a] = json.load(f)
    if (manifests[a]["version"], manifests[a]["commit"]) != (m["version"], m["commit"]):
        raise SystemExit(f"manifest of {a} does not belong to the same version/commit")
built_triples = {}
for a, mm in manifests.items():
    match = re.fullmatch(re.escape(f"{bin_name}-{m['version']}-") + r"(.+)\.tar\.gz", mm["tarball"]["filename"])
    if not match:
        raise SystemExit(f"unexpected tarball name: {mm['tarball']['filename']}")
    built_triples[match.group(1)] = (a, mm["tarball"])

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
    if triple in built_triples:
        ok("tarball", a, [built_triples[triple][1]], triple)
    elif "apple" in triple:
        not_run("tarball", a, APPLE, triple)
    else:
        not_run("tarball", a, secondary.get(a, "architecture not built on this host"), triple)

arches = sorted({arch, "x86_64", "aarch64"})
for a in arches:
    if a in manifests:
        mm = manifests[a]
        ok("oci", a, [
            {"filename": mm["oci"]["archive"], "sha256": mm["oci"]["sha256"]},
            ref(mm["oci"]["archive"].removesuffix(".tar") + ".digest"),
        ])
        ok("sbom", a, [mm["sbom"]["oci_spdx"], mm["sbom"]["oci_cyclonedx"], mm["sbom"]["cargo_spdx"],
                       mm["license_matrix"]])
        ok("provenance", a, [mm["provenance"]])
    else:
        reason = secondary.get(a, "architecture not built on this host")
        for comp in ("oci", "sbom", "provenance"):
            not_run(comp, a, reason)

ok("formula", "all", [ref(formula)])

# The OCI image index (manifest list): one entry per built architecture, each
# pinning the digest of the image manifest INSIDE that architecture's archive.
if len(manifests) > 1:
    import tarfile
    PLATFORM = {"x86_64": "amd64", "aarch64": "arm64"}
    entries = []
    for a in sorted(manifests):
        mm = manifests[a]
        with tarfile.open(os.path.join(out, mm["oci"]["archive"])) as tf:
            idx = json.load(tf.extractfile("index.json"))
        if len(idx["manifests"]) != 1:
            raise SystemExit(f"{a}: the OCI archive must hold exactly one image manifest")
        d = idx["manifests"][0]
        if d["digest"] != mm["oci"].get("archive_manifest_digest"):
            raise SystemExit(f"{a}: manifest oci.archive_manifest_digest differs from the archive index")
        entries.append({
            "mediaType": d["mediaType"],
            "digest": d["digest"],
            "size": d["size"],
            "platform": {"architecture": PLATFORM[a], "os": "linux"},
        })
    index_name = f"{bin_name}-{m['version']}-oci-index.json"
    with open(os.path.join(out, index_name), "w", encoding="utf-8") as f:
        json.dump({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.index.v1+json",
                   "manifests": entries}, f, indent=2)
        f.write("\n")
    ok("oci_index", "all", [ref(index_name)])

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
        "signed_files": ["SHA256SUMS", bundle_name, arch_manifest]
        + [f"{bin_name}-{m['version']}-{a}.manifest.json" for a in extra_arches],
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
  INDEX_NAME="${BIN_NAME}-${VERSION}-oci-index.json"
  EXTRAS=("$FORMULA_NAME" "$NOTES_NAME" "$BUNDLE_NAME")
  [ -f "$INDEX_NAME" ] && EXTRAS+=("$INDEX_NAME")
  for extra in "${EXTRAS[@]}"; do
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
