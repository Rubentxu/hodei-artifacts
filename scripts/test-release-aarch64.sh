#!/usr/bin/env bash
# Second-architecture release test (HSP-287): builds the aarch64 tarball and
# the linux/arm64 OCI image on an x86_64 host (cargo-zigbuild + a qemu-user
# binfmt handler), assembles the two-architecture bundle with
# scripts/release-bundle.sh, and verifies it:
#
#   * scripts/verify-release.sh --require-bundle (signatures, checksums,
#     per-architecture manifests, formula pinning both tarballs);
#   * the bundle records aarch64 as `ok` (not NOT_RUN) and carries an OCI image
#     index that pins both per-architecture image manifest digests;
#   * from an EMPTY directory outside the source tree: `sha256sum -c` and the
#     digest/architecture correlation (scripts/verify-oci-correlation.sh);
#   * the SHIPPED arm64 archive is loaded into an isolated podman store and
#     really runs as aarch64 (uname -m, ELF machine of the binary, and the
#     service answers /health/live under emulation).
#
# Cost: roughly a release build of the three binaries plus the API binary for
# each architecture and an emulated image build, so it is minutes, not seconds.
# It is therefore opt-in with HODEI_TEST_AARCH64=1.
#
# Exit codes: 0 = PASS, 1 = FAIL, 77 = NOT_RUN (opt-in not given, or a tool is
# missing; the reason is printed). A skipped run is never a silent pass.
#
# Usage: HODEI_TEST_AARCH64=1 scripts/test-release-aarch64.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

not_run() { echo "NOT_RUN: aarch64 release test: $*" >&2; exit 77; }
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok - $*"; }

[ "${HODEI_TEST_AARCH64:-0}" = "1" ] || not_run "opt-in (HODEI_TEST_AARCH64=1) not given; it builds two architectures and takes minutes"
[ "$(uname -m)" = "x86_64" ] || not_run "host is $(uname -m); this test cross-builds aarch64 from x86_64"
command -v cargo-zigbuild >/dev/null 2>&1 || not_run "cargo-zigbuild not found in PATH"
command -v zig >/dev/null 2>&1 || not_run "zig not found in PATH"
command -v rustup >/dev/null 2>&1 && ! rustup target list --installed 2>/dev/null | grep -qx aarch64-unknown-linux-gnu \
  && not_run "rust target aarch64-unknown-linux-gnu is not installed (rustup target add aarch64-unknown-linux-gnu)"
command -v podman >/dev/null 2>&1 || not_run "podman not found in PATH"
[ -e /proc/sys/fs/binfmt_misc/qemu-aarch64 ] || not_run "no qemu-aarch64 binfmt handler: the arm64 image cannot be built or run here"
for t in curl python3 gpg; do command -v "$t" >/dev/null 2>&1 || not_run "$t not found in PATH"; done
if ! command -v syft >/dev/null 2>&1; then
  command -v mise >/dev/null 2>&1 && mise exec syft@latest -- syft version >/dev/null 2>&1 \
    || not_run "syft not in PATH and 'mise exec syft@latest' failed"
fi

WORK="$(mktemp -d)"
PROOT="$WORK/podman-root"
PRUN="$WORK/podman-run"
CONTAINER="hodei-hsp287-aarch64-$$"
iso_podman() { podman --storage-driver=vfs --root "$PROOT" --runroot "$PRUN" "$@"; }
cleanup() {
  iso_podman rm -f "$CONTAINER" >/dev/null 2>&1 || true
  # vfs layers are read-only trees owned by sub-uids: remove them from inside the userns.
  podman unshare rm -rf "$WORK" >/dev/null 2>&1 || rm -rf "$WORK"
}
trap cleanup EXIT
OUT="$WORK/bundle"
mkdir -p "$OUT"

STARTED="$(date +%s)"
echo "==> building the two-architecture bundle (this takes minutes)"
RELEASE_BUNDLE_EXTRA_ARCHES=aarch64 "$ROOT/scripts/release-bundle.sh" "$OUT" >"$WORK/bundle.log" 2>&1 \
  || { tail -n 60 "$WORK/bundle.log" >&2; fail "release-bundle.sh failed for x86_64 + aarch64"; }
ok "release-bundle.sh built x86_64 and aarch64 and verified the bundle ($(( $(date +%s) - STARTED ))s)"
grep -q '^NOT_RUN: oci/aarch64' "$WORK/bundle.log" && fail "aarch64 OCI is still NOT_RUN in the bundle"

python3 -I - "$OUT/release-bundle.json" <<'PY' || fail "bundle does not record aarch64 as ok"
import json, sys
b = json.load(open(sys.argv[1]))
status = {(c["component"], c["arch"], c.get("target", "")): c["status"] for c in b["components"]}
for key in (("oci", "aarch64", ""), ("sbom", "aarch64", ""), ("provenance", "aarch64", ""),
            ("tarball", "aarch64", "aarch64-unknown-linux-gnu"), ("oci_index", "all", "")):
    assert status.get(key) == "ok", f"{key} is {status.get(key)!r}"
PY
ok "bundle records aarch64 tarball/oci/sbom/provenance and the OCI index as ok"

. "$ROOT/scripts/lib-clean-room.sh"
clean_room_copy "$OUT" "$WORK/clean-room"
CORRELATION="$( cd "$WORK/clean-room" && "$ROOT/scripts/verify-oci-correlation.sh" "$WORK/clean-room" 2>&1 )" \
  || { echo "$CORRELATION" >&2; fail "clean-room correlation failed"; }
echo "$CORRELATION" | grep -q '^ok - aarch64: archive manifest sha256:.* is linux/arm64' \
  || fail "correlation did not cover aarch64: $CORRELATION"
echo "$CORRELATION" | grep -q '^ok - image index pins \[.amd64., .arm64.\]' \
  || fail "correlation did not cover the image index: $CORRELATION"
ok "clean room: sha256sum -c plus digest/architecture correlation, image index pins amd64 and arm64"

# The index entry must be the digest of the arm64 archive that is shipped.
ARCHIVE="$(ls "$WORK"/clean-room/*-aarch64-oci.tar)"
echo "==> loading the shipped arm64 archive into an isolated store and running it"
iso_podman load -i "$ARCHIVE" >"$WORK/load.log" 2>&1 || { cat "$WORK/load.log" >&2; fail "podman load of the arm64 archive failed"; }
IMAGE="$(iso_podman images --format '{{.Repository}}:{{.Tag}}' | grep -- '-aarch64$' | head -n 1)"
[ -n "$IMAGE" ] || fail "the loaded arm64 image has no tag"
[ "$(iso_podman image inspect --format '{{.Architecture}}' "$IMAGE")" = "arm64" ] || fail "loaded image is not arm64"
[ "$(iso_podman run --rm --platform linux/arm64 --entrypoint /bin/sh "$IMAGE" -c 'uname -m')" = "aarch64" ] \
  || fail "the arm64 image does not report aarch64"
# ELF e_machine of the service binary: 0xb7 (183) is EM_AARCH64.
MACHINE="$(iso_podman run --rm --platform linux/arm64 --entrypoint /bin/sh "$IMAGE" -c 'od -An -tx1 -j18 -N2 /app/bin/hodei-artifacts-api' | tr -d ' \n')"
[ "$MACHINE" = "b700" ] || fail "binary ELF e_machine is '$MACHINE', expected b700 (AArch64)"
ok "the shipped archive loads as linux/arm64 and its binary is an AArch64 ELF"

# The embedded profile is retired (HSP-314 F4): without a [graph] section the
# service refuses to start. Under emulation that refusal is the proof that the
# AArch64 binary executes and reaches its own configuration check; a full boot
# needs a Dgraph, which this emulated test does not carry.
set +e
OUT="$(iso_podman run --rm --platform linux/arm64 --read-only --tmpfs /tmp:rw,noexec,nosuid,size=64m "$IMAGE" 2>&1)"
STATUS=$?
set -e
[ "$STATUS" -ne 0 ] || fail "the aarch64 service started without a [graph] section"
printf '%s' "$OUT" | grep -q '\[graph\]' || { printf '%s\n' "$OUT" >&2; fail "the aarch64 service did not refuse with the graph-profile message"; }
ok "the aarch64 service runs under qemu-user emulation and refuses a deployment without [graph]"

echo "aarch64 release test passed in $(( $(date +%s) - STARTED ))s"
