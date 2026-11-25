#!/usr/bin/env bash
# Installs the prebuilt `hodei-artifacts-api` binaries after verifying them.
#
# Nothing is copied into the install directory until both checks pass:
#   1. the OpenPGP detached signatures of SHA256SUMS and the release manifest
#      (verified with gpgv against the bundled public key), and
#   2. the SHA-256 of the tarball, listed in the signed SHA256SUMS and repeated
#      in the signed manifest.
#
# The bundle layout is the one produced by scripts/package-release.sh and
# scripts/package-oci.sh. The signing key travels inside the bundle (it is an
# ephemeral per-release key), so by default the signature proves integrity of
# the bundle, not the identity of the publisher. Pin the publisher with
# --fingerprint to close that gap.
#
# Usage:
#   install.sh --bundle-dir DIR [options]       read a local bundle (no network)
#   install.sh --version X.Y.Z [options]        download from the release URL
#
# Options:
#   --bundle-dir DIR     local directory with the release assets
#   --version VERSION    release version to download (without leading "v")
#   --base-url URL       download base URL (default: GitHub release of VERSION)
#   --target TRIPLE      override the detected target triple
#   --bin-dir DIR        install directory (default: ~/.local/bin, or
#                        /usr/local/bin when run as root)
#   --config-dir DIR     where to initialise config (default: ~/.config/hodei-artifacts)
#   --fingerprint FPR    require the bundled key to have this OpenPGP fingerprint
#   --no-init            do not initialise the configuration
#   -h, --help           show this help

set -euo pipefail

BIN_NAME="hodei-artifacts-api"
DEFAULT_REPO_URL="https://github.com/Rubentxu/hodei-artifacts"

BUNDLE_DIR=""
VERSION=""
BASE_URL=""
TARGET=""
BIN_DIR=""
CONFIG_DIR=""
FINGERPRINT=""
INIT_CONFIG=1

die() { echo "error: $*" >&2; exit 1; }

usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --bundle-dir) BUNDLE_DIR="${2:?--bundle-dir needs a value}"; shift 2 ;;
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --base-url) BASE_URL="${2:?--base-url needs a value}"; shift 2 ;;
    --target) TARGET="${2:?--target needs a value}"; shift 2 ;;
    --bin-dir) BIN_DIR="${2:?--bin-dir needs a value}"; shift 2 ;;
    --config-dir) CONFIG_DIR="${2:?--config-dir needs a value}"; shift 2 ;;
    --fingerprint) FINGERPRINT="${2:?--fingerprint needs a value}"; shift 2 ;;
    --no-init) INIT_CONFIG=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1 (see --help)" ;;
  esac
done

if [ -z "$BUNDLE_DIR" ] && [ -z "$VERSION" ]; then
  die "pass --bundle-dir DIR (local) or --version X.Y.Z (download)"
fi
if [ -n "$BUNDLE_DIR" ] && [ ! -d "$BUNDLE_DIR" ]; then
  die "bundle directory not found: $BUNDLE_DIR"
fi

for tool in tar gpg gpgv python3; do
  command -v "$tool" >/dev/null 2>&1 || die "required tool not found: $tool"
done

# SHA-256 digest of a file. GNU coreutils ships sha256sum; macOS ships only
# shasum, so fall back to `shasum -a 256` when sha256sum is absent.
if command -v sha256sum >/dev/null 2>&1; then
  sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "required tool not found: sha256sum (or shasum)"
fi

# --- Platform detection -----------------------------------------------------

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) die "unsupported architecture: $arch" ;;
  esac
  case "$os" in
    Linux) echo "${arch}-unknown-linux-gnu" ;;
    Darwin) echo "${arch}-apple-darwin" ;;
    *) die "unsupported operating system: $os" ;;
  esac
}

[ -n "$TARGET" ] || TARGET="$(detect_target)"

if [ -z "$BIN_DIR" ]; then
  if [ "$(id -u)" -eq 0 ]; then
    BIN_DIR="/usr/local/bin"
  else
    BIN_DIR="$HOME/.local/bin"
  fi
fi
[ -n "$CONFIG_DIR" ] || CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/hodei-artifacts"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
STAGE="$WORK_DIR/bundle"
mkdir -p "$STAGE"

# --- Obtain the bundle ------------------------------------------------------

fetch() { # <name>
  if [ -n "$BUNDLE_DIR" ]; then
    [ -f "$BUNDLE_DIR/$1" ] || die "bundle asset missing: $BUNDLE_DIR/$1"
    cp "$BUNDLE_DIR/$1" "$STAGE/$1"
  else
    command -v curl >/dev/null 2>&1 || die "required tool not found: curl"
    curl --proto '=https' --tlsv1.2 -fsSL "$BASE_URL/$1" -o "$STAGE/$1" \
      || die "download failed: $BASE_URL/$1"
  fi
}

if [ -z "$BUNDLE_DIR" ] && [ -z "$BASE_URL" ]; then
  BASE_URL="$DEFAULT_REPO_URL/releases/download/v${VERSION}"
fi

fetch SHA256SUMS
fetch SHA256SUMS.asc
fetch release-signing-key.asc

# The tarball is named after the version and target; when only a local bundle
# is given, take the version from the (still unverified) tarball name.
if [ -z "$VERSION" ]; then
  shopt -s nullglob
  # shellcheck disable=SC2206 # the glob is intentional
  candidates=("$BUNDLE_DIR"/${BIN_NAME}-*-"${TARGET}".tar.gz)
  shopt -u nullglob
  [ "${#candidates[@]}" -eq 1 ] || die "expected exactly one ${BIN_NAME}-*-${TARGET}.tar.gz in $BUNDLE_DIR"
  base="$(basename "${candidates[0]}")"
  VERSION="${base#"${BIN_NAME}-"}"
  VERSION="${VERSION%"-${TARGET}.tar.gz"}"
fi

PKG_NAME="${BIN_NAME}-${VERSION}-${TARGET}"
TARBALL="${PKG_NAME}.tar.gz"

# The manifest is named after the machine architecture (uname -m), not the
# full triple.
MANIFEST="$(grep -E "^[0-9a-f]{64}  ${BIN_NAME}-${VERSION}-${TARGET%%-*}\.manifest\.json$" "$STAGE/SHA256SUMS" | head -n 1 | sed 's/^[0-9a-f]*  //' || true)"
[ -n "$MANIFEST" ] || die "SHA256SUMS does not list a release manifest"

fetch "$TARBALL"
fetch "$MANIFEST"
fetch "${MANIFEST}.asc"

# --- Verification (before anything is installed) ----------------------------

if [ -n "$FINGERPRINT" ]; then
  want="$(printf '%s' "$FINGERPRINT" | tr -d ' ' | tr '[:lower:]' '[:upper:]')"
  # A keyring with several keys would let any of them satisfy gpgv, so a
  # pinned publisher requires a bundle key file with exactly one primary key.
  keys="$(gpg --show-keys --with-colons "$STAGE/release-signing-key.asc" 2>/dev/null)"
  [ "$(printf '%s\n' "$keys" | grep -c '^pub:')" -eq 1 ] \
    || die "signing key file must contain exactly one primary key when --fingerprint is used"
  have="$(printf '%s\n' "$keys" | awk -F: '$1=="fpr"{print toupper($10); exit}')"
  [ "$have" = "$want" ] || die "signing key fingerprint mismatch (expected $want, bundle has ${have:-none})"
fi

gpg --dearmor < "$STAGE/release-signing-key.asc" > "$STAGE/trusted.gpg" 2>/dev/null \
  || die "release-signing-key.asc is not a valid OpenPGP public key"

echo "==> verifying OpenPGP signatures"
verify_sig() { # <signature> <file> <label>
  local status
  status="$(gpgv --status-fd 1 --keyring "$STAGE/trusted.gpg" "$1" "$2" 2>/dev/null)" \
    || die "$3 signature verification failed"
  if [ -n "$FINGERPRINT" ]; then
    # The last VALIDSIG field is the primary key fingerprint of the signer.
    printf '%s\n' "$status" | awk -v w="$want" '$2=="VALIDSIG" && toupper($NF)==w {ok=1} END{exit !ok}' \
      || die "$3 was not signed by the pinned signing key"
  fi
}
verify_sig "$STAGE/SHA256SUMS.asc" "$STAGE/SHA256SUMS" "SHA256SUMS"
verify_sig "$STAGE/${MANIFEST}.asc" "$STAGE/$MANIFEST" "manifest"

echo "==> verifying SHA-256"
expected="$(grep -E "^[0-9a-f]{64}  ${TARBALL}$" "$STAGE/SHA256SUMS" | head -n 1 | cut -d' ' -f1 || true)"
[ -n "$expected" ] || die "SHA256SUMS does not list $TARBALL"
actual="$(sha256_of "$STAGE/$TARBALL")"
[ "$actual" = "$expected" ] || die "checksum mismatch for $TARBALL"
manifest_sha="$(grep -E "^[0-9a-f]{64}  ${MANIFEST}$" "$STAGE/SHA256SUMS" | cut -d' ' -f1 || true)"
[ "$(sha256_of "$STAGE/$MANIFEST")" = "$manifest_sha" ] || die "checksum mismatch for $MANIFEST"

python3 -I - "$STAGE/$MANIFEST" "$TARBALL" "$expected" <<'PY' || die "manifest does not match the tarball"
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
t = m.get("tarball", {})
if t.get("filename") != sys.argv[2] or t.get("sha256") != sys.argv[3]:
    raise SystemExit(1)
PY

# --- Extraction (path-safe) and install -------------------------------------

while IFS= read -r entry; do
  case "$entry" in
    /*|*..*) die "unsafe path in tarball: $entry" ;;
  esac
  case "$entry" in
    "$PKG_NAME"|"$PKG_NAME"/*) ;;
    *) die "unexpected entry outside $PKG_NAME/: $entry" ;;
  esac
done < <(tar -tzf "$STAGE/$TARBALL")

mkdir -p "$WORK_DIR/extract"
tar -xzf "$STAGE/$TARBALL" -C "$WORK_DIR/extract" --no-same-owner
SRC="$WORK_DIR/extract/$PKG_NAME"
[ -x "$SRC/bin/$BIN_NAME" ] || die "$BIN_NAME is missing from the package"

mkdir -p "$BIN_DIR"
for f in "$SRC"/bin/*; do
  install -m 0755 "$f" "$BIN_DIR/$(basename "$f")"
done
echo "==> installed to $BIN_DIR: $(cd "$SRC/bin" && echo *)"

if [ "$INIT_CONFIG" -eq 1 ]; then
  mkdir -p "$CONFIG_DIR"
  for f in default.toml release.toml; do
    if [ -f "$SRC/config/$f" ] && [ ! -e "$CONFIG_DIR/$f" ]; then
      install -m 0644 "$SRC/config/$f" "$CONFIG_DIR/$f"
    fi
  done
  echo "==> configuration initialised in $CONFIG_DIR (existing files are kept)"
  echo "    the service reads config/default.toml relative to its working directory;"
  echo "    run it from a directory that contains a config/ folder (e.g. copy $CONFIG_DIR to ./config)."
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: $BIN_DIR is not in PATH" >&2 ;;
esac
echo "OK: $BIN_NAME $VERSION ($TARGET) verified and installed"
