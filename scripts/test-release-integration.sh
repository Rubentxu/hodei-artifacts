#!/usr/bin/env bash
# Integration test of the real release path (HSP-272).
#
# Unlike scripts/test-install.sh (synthetic bundle, stub binaries), this script
# packages a bundle with the real scripts/package-release.sh and
# scripts/package-oci.sh and installs it with scripts/install.sh:
#
#   Stage A (needs only cargo/rustc): package-release.sh builds the tarball and
#     the cargo-binstall `pkg-url` / `bin-dir` templates of Cargo.toml are
#     expanded with the values package-release.sh uses and compared with the
#     real artifact name and layout.
#   Stage B (needs podman and syft): package-oci.sh produces the signed bundle
#     (manifest, SHA256SUMS, signatures); install.sh installs it, and the
#     Homebrew formula is generated from the real manifest.
#
# Exit codes: 0 = PASS, 1 = FAIL, 77 = NOT_RUN (stage B tool missing; stage A
# has already run and its result was printed). A missing tool is never a
# silent pass.
#
# Usage: scripts/test-release-integration.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
# HSP-314: la etapa B construye la imagen en un almacen de podman propio de
# esta corrida (PODMAN_ROOT/RUNROOT dentro de $WORK), para no retaguear
# `:latest` del almacen compartido (/var/tmp/podman-root). Para reutilizar
# otro almacen, exporta HODEI_RELEASE_TEST_SHARED_PODMAN=1.
if [ "${HODEI_RELEASE_TEST_SHARED_PODMAN:-0}" != "1" ]; then
  export PODMAN_ROOT="$WORK/podman-root" PODMAN_RUNROOT="$WORK/podman-runroot"
  mkdir -p "$PODMAN_ROOT" "$PODMAN_RUNROOT"
fi
# Los ficheros del almacen vfs pueden pertenecer a subuids: se borran dentro
# del user namespace de podman, y si no se puede, con rm normal.
cleanup() {
  if [ -d "$WORK/podman-root" ]; then
    podman unshare rm -rf "$WORK/podman-root" "$WORK/podman-runroot" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT
OUT="$WORK/dist"
mkdir -p "$OUT"

PASS=0
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { PASS=$((PASS + 1)); echo "ok - $*"; }

BINS=(hodei-artifacts-api)
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
TARGET="$(rustc -vV | sed -n 's/^host: //p')"

# --- Stage A: real package-release.sh + binstall template ---------------------

echo "==> stage A: package-release.sh (debug profile, same layout as release)"
HODEI_PACKAGE_PROFILE=debug "$ROOT/scripts/package-release.sh" "$OUT" >"$WORK/package-release.log" 2>&1 \
  || { cat "$WORK/package-release.log" >&2; fail "package-release.sh failed"; }

ARTIFACTS=("$OUT"/*.tar.gz)
[ "${#ARTIFACTS[@]}" -eq 1 ] || fail "expected exactly one tarball, found: ${ARTIFACTS[*]}"
REAL_NAME="$(basename "${ARTIFACTS[0]}")"

# Expand the binstall templates with the values cargo-binstall would use:
# repo/name/version from [package], target = rustc host triple, and
# archive-format = tar.gz for pkg-fmt "tgz".
EXPANDED="$(python3 -I - "$ROOT/Cargo.toml" "$TARGET" <<'PY'
import sys, tomllib
cargo = tomllib.load(open(sys.argv[1], "rb"))
pkg = cargo["package"]
binstall = pkg["metadata"]["binstall"]
fmt = {"tgz": "tar.gz", "txz": "tar.xz", "tbz2": "tar.bz2", "zip": "zip"}[binstall["pkg-fmt"]]
values = {
    "repo": pkg["repository"].rstrip("/"),
    "name": pkg["name"],
    "version": pkg["version"],
    "target": sys.argv[2],
    "archive-format": fmt,
    "binary-ext": "",
}
def expand(template, extra=None):
    merged = dict(values, **(extra or {}))
    out = template
    for key, val in merged.items():
        out = out.replace("{ %s }" % key, val).replace("{%s}" % key, val)
    assert "{ " not in out, "unexpanded placeholder in " + out
    return out
print("PKG_URL=" + expand(binstall["pkg-url"]))
for b in ("hodei-artifacts-api",):
    print("BIN_DIR=" + expand(binstall["bin-dir"], {"bin": b}))
PY
)"
PKG_URL="$(printf '%s\n' "$EXPANDED" | sed -n 's/^PKG_URL=//p')"
[ "$(basename "$PKG_URL")" = "$REAL_NAME" ] \
  || fail "binstall pkg-url expands to '$(basename "$PKG_URL")' but package-release.sh produced '$REAL_NAME'"
case "$PKG_URL" in
  *"/releases/download/v${VERSION}/${REAL_NAME}") ;;
  *) fail "binstall pkg-url does not follow the release download layout: $PKG_URL" ;;
esac
ok "binstall pkg-url expands to the real artifact name ($REAL_NAME)"

LISTING="$(tar -tzf "$OUT/$REAL_NAME")"
while IFS= read -r path; do
  printf '%s\n' "$LISTING" | grep -qx "$path" \
    || fail "binstall bin-dir path not found in the real tarball: $path"
done < <(printf '%s\n' "$EXPANDED" | sed -n 's/^BIN_DIR=//p')
ok "binstall bin-dir paths exist inside the real tarball"

# --- Stage B: real package-oci.sh + install.sh --------------------------------

missing=""
command -v podman >/dev/null 2>&1 || missing="podman not found in PATH"
if [ -z "$missing" ] && ! command -v syft >/dev/null 2>&1; then
  if command -v mise >/dev/null 2>&1; then
    mise exec syft@latest -- syft version >/dev/null 2>&1 || missing="syft not in PATH and 'mise exec syft@latest' failed"
  else
    missing="syft not in PATH and mise is unavailable"
  fi
fi
if [ -z "$missing" ]; then
  podman --storage-driver="${PODMAN_STORAGE_DRIVER:-vfs}" --root "${PODMAN_ROOT:-/var/tmp/podman-root}" \
    --runroot "${PODMAN_RUNROOT:-/var/tmp/podman-runroot}" info >/dev/null 2>&1 \
    || missing="podman is installed but 'podman info' fails"
fi
if [ -n "$missing" ]; then
  echo "NOT_RUN: stage B (package-oci.sh + install.sh) skipped: $missing" >&2
  echo "stage A passed ($PASS checks); stage B is NOT_RUN" >&2
  exit 77
fi

echo "==> stage B: package-oci.sh (builds the OCI image and signs the bundle)"
"$ROOT/scripts/package-oci.sh" "$OUT" >"$WORK/package-oci.log" 2>&1 \
  || { tail -n 40 "$WORK/package-oci.log" >&2; fail "package-oci.sh failed"; }
for f in SHA256SUMS SHA256SUMS.asc release-signing-key.asc "$REAL_NAME"; do
  [ -f "$OUT/$f" ] || fail "bundle asset missing after package-oci.sh: $f"
done
ok "package-oci.sh produced the signed bundle"

echo "==> installing the real bundle with install.sh"
"$ROOT/scripts/install.sh" --bundle-dir "$OUT" --target "$TARGET" \
  --bin-dir "$WORK/inst/bin" --config-dir "$WORK/inst/cfg" >"$WORK/install.log" 2>&1 \
  || { cat "$WORK/install.log" >&2; fail "install.sh rejected the real bundle"; }
for b in "${BINS[@]}"; do
  [ -x "$WORK/inst/bin/$b" ] || fail "binary not installed from the real bundle: $b"
done
[ -f "$WORK/inst/cfg/default.toml" ] || fail "configuration not initialised from the real bundle"
# HSP-314: the release ships the service alone; a legacy tool in bin/ would
# mean the tarball was built with the embedded engine.
[ "$(ls "$WORK/inst/bin")" = "hodei-artifacts-api" ] \
  || fail "the installed bin/ must hold only hodei-artifacts-api, found: $(ls "$WORK/inst/bin" | tr '\n' ' ')"
ok "install.sh installs the real bundle (binaries and config)"

# The same bundle with a tampered tarball must be rejected.
cp -r "$OUT" "$WORK/tampered"
echo x >> "$WORK/tampered/$REAL_NAME"
if "$ROOT/scripts/install.sh" --bundle-dir "$WORK/tampered" --target "$TARGET" \
  --bin-dir "$WORK/tinst/bin" --config-dir "$WORK/tinst/cfg" >"$WORK/tamper.log" 2>&1; then
  fail "install.sh accepted a tampered real bundle"
fi
grep -q "checksum mismatch" "$WORK/tamper.log" || fail "tamper rejection reason unexpected: $(cat "$WORK/tamper.log")"
[ ! -e "$WORK/tinst/bin" ] || fail "files installed despite the tampered tarball"
ok "tampered real bundle rejected"

# Homebrew formula from the real manifest.
MANIFEST="$(find "$OUT" -maxdepth 1 -name '*.manifest.json' | head -n 1)"
[ -n "$MANIFEST" ] || fail "real manifest not found"
SHA="$(sha256sum "$OUT/$REAL_NAME" | cut -d' ' -f1)"
FORMULA_OUT="$("$ROOT/scripts/generate-homebrew-formula.sh" "$MANIFEST")" || fail "formula generation failed on the real manifest"
echo "$FORMULA_OUT" | grep -q "sha256 \"$SHA\"" || fail "formula sha256 does not match the real tarball"
echo "$FORMULA_OUT" | grep -q "version \"$VERSION\"" || fail "formula version does not match Cargo.toml"
ok "Homebrew formula from the real manifest carries the real sha256"

# --- Stage C: clean-room verification of the served assets (HSP-287) ---------
#
# The bundle is copied to an EMPTY directory outside the source tree and checked
# from there with nothing but sha256sum/tar/python3: checksums, and the
# correlation of every digest and architecture with the manifest.
# (The second architecture is stage D, scripts/test-release-aarch64.sh.)

echo "==> stage C: clean-room checksum and digest/architecture correlation"
. "$ROOT/scripts/lib-clean-room.sh"
clean_room_copy "$OUT" "$WORK/clean-room"
case "$WORK/clean-room" in "$ROOT"/*) fail "the clean room is inside the source tree" ;; esac
( cd "$WORK/clean-room" && "$ROOT/scripts/verify-oci-correlation.sh" "$WORK/clean-room" ) >"$WORK/correlation.log" 2>&1 \
  || { cat "$WORK/correlation.log" >&2; fail "clean-room correlation failed on the real bundle"; }
grep -q "^ok - $(uname -m): archive manifest sha256:" "$WORK/correlation.log" \
  || fail "correlation output does not report the host architecture: $(cat "$WORK/correlation.log")"
ok "clean room: sha256sum -c and digest/architecture correlation pass"

# A byte appended to the OCI archive breaks the checksum.
clean_room_copy "$OUT" "$WORK/clean-tamper"
ARCHIVE_NAME="$(basename "$(ls "$WORK"/clean-tamper/*-oci.tar)")"
echo x >> "$WORK/clean-tamper/$ARCHIVE_NAME"
if "$ROOT/scripts/verify-oci-correlation.sh" "$WORK/clean-tamper" >"$WORK/tamper-c.log" 2>&1; then
  fail "clean room accepted a tampered OCI archive"
fi
grep -q "FAILED" "$WORK/tamper-c.log" || fail "tampered archive rejected for an unexpected reason: $(cat "$WORK/tamper-c.log")"
ok "clean room rejects a tampered OCI archive"

# A manifest that claims the other architecture is rejected even when the
# checksums are recomputed to match (the correlation, not the checksum, fails).
clean_room_copy "$OUT" "$WORK/clean-mislabel"
MANIFEST_NAME="$(basename "$(ls "$WORK"/clean-mislabel/*.manifest.json)")"
python3 -I - "$WORK/clean-mislabel/$MANIFEST_NAME" <<'PY'
import json, sys
path = sys.argv[1]
m = json.load(open(path))
m["arch"] = "aarch64" if m["arch"] == "x86_64" else "x86_64"
json.dump(m, open(path, "w"))
PY
( cd "$WORK/clean-mislabel" && sha256sum "$MANIFEST_NAME" > "$WORK/new.sum" \
  && grep -v " $MANIFEST_NAME\$" SHA256SUMS > SHA256SUMS.new && cat "$WORK/new.sum" >> SHA256SUMS.new \
  && mv SHA256SUMS.new SHA256SUMS )
if "$ROOT/scripts/verify-oci-correlation.sh" "$WORK/clean-mislabel" >"$WORK/mislabel.log" 2>&1; then
  fail "clean room accepted a manifest with the wrong architecture"
fi
grep -q "^error: " "$WORK/mislabel.log" || fail "mislabel rejected for an unexpected reason: $(cat "$WORK/mislabel.log")"
ok "clean room rejects a manifest whose architecture does not match the image"

echo "all $PASS checks passed"
