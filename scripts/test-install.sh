#!/usr/bin/env bash
# Automated suite for scripts/install.sh and scripts/generate-homebrew-formula.sh.
#
# Builds a signed release bundle from scratch in a clean temporary directory
# (ephemeral OpenPGP key, stub binaries, no network) with the same layout as
# scripts/package-release.sh and scripts/package-oci.sh, then checks that the
# installer accepts it and rejects tampered bundles without installing anything.
#
# Usage: scripts/test-install.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL="$ROOT/scripts/install.sh"
FORMULA="$ROOT/scripts/generate-homebrew-formula.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export GNUPGHOME="$WORK/gnupg"
mkdir -p "$GNUPGHOME" && chmod 700 "$GNUPGHOME"

VERSION="9.9.9"
TARGET="x86_64-unknown-linux-gnu"
PKG="hodei-artifacts-api-${VERSION}-${TARGET}"
MANIFEST="hodei-artifacts-api-${VERSION}-x86_64.manifest.json"

PASS=0
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { PASS=$((PASS + 1)); echo "ok - $*"; }

gpg --batch --quiet --passphrase '' --quick-generate-key "test <test@example.invalid>" ed25519 sign never 2>/dev/null
FPR="$(gpg --list-keys --with-colons | awk -F: '$1=="fpr"{print $10; exit}')"

# build_bundle <dir> [tar-extra-entry]
build_bundle() {
  local dir="$1" evil="${2:-}"
  # shellcheck disable=SC2206 # space-separated fingerprints, intentional
  local export_fprs=(${EXPORT_FPRS:-$FPR})
  rm -rf "$dir" "$WORK/pkg"
  mkdir -p "$dir" "$WORK/pkg/$PKG/bin" "$WORK/pkg/$PKG/config"
  # HSP-314: the release ships the service alone (the legacy operator tools
  # live in scripts/package-legacy-tools.sh); hodei-aux stands in for any
  # extra file under bin/ so the multi-file install path stays covered.
  for b in hodei-artifacts-api hodei-aux; do
    printf '#!/bin/sh\necho %s\n' "$b" > "$WORK/pkg/$PKG/bin/$b"
    chmod 0755 "$WORK/pkg/$PKG/bin/$b"
  done
  echo '[server]' > "$WORK/pkg/$PKG/config/default.toml"
  echo '[server]' > "$WORK/pkg/$PKG/config/release.toml"
  if [ -n "$evil" ]; then
    tar -C "$WORK/pkg" -czf "$dir/$PKG.tar.gz" --transform "s|^$PKG/bin/hodei-aux|$evil|" "$PKG"
  else
    tar -C "$WORK/pkg" -czf "$dir/$PKG.tar.gz" "$PKG"
  fi
  local sha
  sha="$(sha256sum "$dir/$PKG.tar.gz" | cut -d' ' -f1)"
  printf '{"name":"hodei-artifacts-api","version":"%s","arch":"x86_64","tarball":{"filename":"%s.tar.gz","sha256":"%s"}}\n' \
    "$VERSION" "$PKG" "$sha" > "$dir/$MANIFEST"
  gpg --batch --quiet --armor --export "${export_fprs[@]}" > "$dir/release-signing-key.asc"
  ( cd "$dir" && sha256sum "$PKG.tar.gz" "$MANIFEST" > SHA256SUMS )
  gpg --batch --quiet --detach-sign --armor --local-user "${SIGNER:-$FPR}" -o "$dir/SHA256SUMS.asc" "$dir/SHA256SUMS"
  gpg --batch --quiet --detach-sign --armor --local-user "${SIGNER:-$FPR}" -o "$dir/$MANIFEST.asc" "$dir/$MANIFEST"
}

run_install() { # <bundle> <name> [extra args]; installs under $WORK/<name>
  local bundle="$1" name="$2"; shift 2
  rm -rf "${WORK:?}/$name"
  "$INSTALL" --bundle-dir "$bundle" --target "$TARGET" \
    --bin-dir "$WORK/$name/bin" --config-dir "$WORK/$name/cfg" "$@"
}

expect_reject() { # <bundle> <label> <message-fragment> [extra args]
  local bundle="$1" label="$2" frag="$3"; shift 3
  local out
  if out="$(run_install "$bundle" "rej" "$@" 2>&1)"; then
    fail "$label: installer accepted a bad bundle"
  fi
  echo "$out" | grep -q "$frag" || fail "$label: expected '$frag' in: $out"
  [ ! -e "$WORK/rej/bin" ] || fail "$label: files were installed despite the failure"
  ok "$label rejected"
}

# 1. Happy path (version detected from the local bundle).
build_bundle "$WORK/good"
run_install "$WORK/good" inst >/dev/null
for b in hodei-artifacts-api hodei-aux; do
  [ -x "$WORK/inst/bin/$b" ] || fail "binary not installed: $b"
done
[ -f "$WORK/inst/cfg/default.toml" ] || fail "configuration not initialised"
ok "verified bundle installs binaries and initialises config"

# 2. Existing configuration is kept; --no-init skips it.
echo 'custom' > "$WORK/inst/cfg/default.toml"
"$INSTALL" --bundle-dir "$WORK/good" --target "$TARGET" --bin-dir "$WORK/inst/bin" --config-dir "$WORK/inst/cfg" >/dev/null
[ "$(cat "$WORK/inst/cfg/default.toml")" = "custom" ] || fail "existing config was overwritten"
run_install "$WORK/good" noinit --no-init >/dev/null
[ ! -e "$WORK/noinit/cfg" ] || fail "--no-init still created the config"
ok "config preserved and --no-init honoured"

# 3. Pinned fingerprint.
run_install "$WORK/good" pin --fingerprint "$FPR" >/dev/null
ok "matching fingerprint accepted"
expect_reject "$WORK/good" "wrong fingerprint" "fingerprint mismatch" --fingerprint "0000000000000000000000000000000000000000"

# 4. Tampered tarball (checksum mismatch).
build_bundle "$WORK/t1"; echo x >> "$WORK/t1/$PKG.tar.gz"
expect_reject "$WORK/t1" "tampered tarball" "checksum mismatch"

# 5. Tampered SHA256SUMS (signature failure).
build_bundle "$WORK/t2"; sed -i 's/^0/1/;t;s/^[1-9a-f]/0/' "$WORK/t2/SHA256SUMS"
expect_reject "$WORK/t2" "tampered SHA256SUMS" "SHA256SUMS signature"

# 6. Tampered manifest (signature failure).
build_bundle "$WORK/t3"; echo ' ' >> "$WORK/t3/$MANIFEST"
expect_reject "$WORK/t3" "tampered manifest" "manifest signature"

# 7. Consistent tarball+SHA256SUMS re-signed with a different key but a
#    pinned publisher fingerprint: the substitution is caught.
build_bundle "$WORK/t4"
OLD_FPR="$FPR"
gpg --batch --quiet --passphrase '' --quick-generate-key "other <other@example.invalid>" ed25519 sign never 2>/dev/null
FPR="$(gpg --list-keys --with-colons other | awk -F: '$1=="fpr"{print $10; exit}')"
build_bundle "$WORK/t4"
expect_reject "$WORK/t4" "bundle re-signed by another key" "fingerprint mismatch" --fingerprint "$OLD_FPR"
ATTACKER_FPR="$FPR"
FPR="$OLD_FPR"

# 7b. Regression: a multi-key keyring [legitimate + attacker] signed by the
#     attacker must not satisfy a pinned legitimate fingerprint.
EXPORT_FPRS="$OLD_FPR $ATTACKER_FPR" SIGNER="$ATTACKER_FPR" build_bundle "$WORK/t4b"
expect_reject "$WORK/t4b" "multi-key keyring signed by attacker" "fingerprint\|signing key" --fingerprint "$OLD_FPR"

# 8. Missing signature file.
build_bundle "$WORK/t5"; rm "$WORK/t5/SHA256SUMS.asc"
expect_reject "$WORK/t5" "missing signature" "bundle asset missing"

# 9. Correctly signed tarball with a path escaping the package directory.
build_bundle "$WORK/t6" "../escape"
expect_reject "$WORK/t6" "path traversal in tarball" "unsafe path"

# 9b. macOS portability: no sha256sum in PATH, only `shasum`. A synthetic PATH
#     links every tool of /usr/bin except sha256sum, and offers `shasum` (the
#     real one when present, otherwise a wrapper over the real sha256sum that
#     speaks `shasum -a 256`). The digest check must still pass for a good
#     bundle and still fail for a tampered one.
NOSHA="$WORK/nosha-bin"
mkdir -p "$NOSHA"
for f in /usr/bin/*; do
  [ "$(basename "$f")" = "sha256sum" ] || ln -s "$f" "$NOSHA/$(basename "$f")" 2>/dev/null || true
done
if [ ! -e "$NOSHA/shasum" ]; then
  REAL_SHA256SUM="$(command -v sha256sum)"
  # shellcheck disable=SC2016 # literal wrapper script, expanded when it runs
  printf '#!/bin/sh\n[ "$1" = "-a" ] && [ "$2" = "256" ] || exit 2\nshift 2\nexec %s "$@"\n' "$REAL_SHA256SUM" > "$NOSHA/shasum"
  chmod 0755 "$NOSHA/shasum"
fi
[ -x "$NOSHA/shasum" ] || fail "synthetic PATH lacks shasum"
if PATH="$NOSHA" command -v sha256sum >/dev/null 2>&1; then fail "synthetic PATH still has sha256sum"; fi
build_bundle "$WORK/good"
rm -rf "$WORK/nosha"
PATH="$NOSHA" /usr/bin/env bash "$INSTALL" --bundle-dir "$WORK/good" --target "$TARGET" \
  --bin-dir "$WORK/nosha/bin" --config-dir "$WORK/nosha/cfg" >/dev/null \
  || fail "installer failed without sha256sum (shasum fallback)"
[ -x "$WORK/nosha/bin/hodei-artifacts-api" ] || fail "no binary installed via the shasum fallback"
ok "installer verifies SHA-256 with the shasum fallback when sha256sum is absent"
build_bundle "$WORK/t7"; echo x >> "$WORK/t7/$PKG.tar.gz"
rm -rf "$WORK/nosha-rej"
if out="$(PATH="$NOSHA" /usr/bin/env bash "$INSTALL" --bundle-dir "$WORK/t7" --target "$TARGET" \
  --bin-dir "$WORK/nosha-rej/bin" --config-dir "$WORK/nosha-rej/cfg" 2>&1)"; then
  fail "shasum fallback accepted a tampered tarball"
fi
echo "$out" | grep -q "checksum mismatch" || fail "shasum fallback: expected checksum mismatch in: $out"
[ ! -e "$WORK/nosha-rej/bin" ] || fail "shasum fallback installed files despite the mismatch"
ok "tampered tarball still rejected with the shasum fallback"

# 10. Homebrew formula from the manifest: real URL, version and sha256.
build_bundle "$WORK/good"
SHA="$(sha256sum "$WORK/good/$PKG.tar.gz" | cut -d' ' -f1)"
FORMULA_OUT="$("$FORMULA" "$WORK/good/$MANIFEST")"
echo "$FORMULA_OUT" | grep -q "class HodeiArtifactsApi < Formula" || fail "formula class"
echo "$FORMULA_OUT" | grep -q "version \"$VERSION\"" || fail "formula version"
echo "$FORMULA_OUT" | grep -q "sha256 \"$SHA\"" || fail "formula sha256"
echo "$FORMULA_OUT" | grep -q "url \"https://github.com/Rubentxu/hodei-artifacts/releases/download/v$VERSION/$PKG.tar.gz\"" || fail "formula url"
ok "Homebrew formula carries version, url and sha256 from the manifest"
sed -i 's/"sha256":"[0-9a-f]*"/"sha256":"nothex"/' "$WORK/good/$MANIFEST"
if "$FORMULA" "$WORK/good/$MANIFEST" >/dev/null 2>&1; then fail "formula accepted an invalid sha256"; fi
ok "Homebrew generator rejects an invalid manifest"

# 11. Cargo.toml binstall metadata matches the package-release.sh layout.
grep -q '^\[package.metadata.binstall\]' "$ROOT/Cargo.toml" || fail "binstall metadata missing"
grep -q 'pkg-url = ".*{ name }-{ version }-{ target }\.{ archive-format }"' "$ROOT/Cargo.toml" || fail "binstall pkg-url"
grep -q 'bin-dir = "{ name }-{ version }-{ target }/bin/' "$ROOT/Cargo.toml" || fail "binstall bin-dir"
# shellcheck disable=SC2016 # literal match of the shell source
grep -q 'PKG_NAME="${BIN_NAME}-${VERSION}-${TARGET}"' "$ROOT/scripts/package-release.sh" || fail "package-release.sh naming drifted"
ok "binstall metadata matches package-release.sh layout"

echo "all $PASS checks passed"
