#!/usr/bin/env bash
# Builds the reproducible OCI container image and correlated release manifest
# for `hodei-artifacts-api`.
#
# Produces in <output-directory> (default: ./dist):
#   hodei-artifacts-api-<version>-<arch>-oci.tar
#   hodei-artifacts-api-<version>-<arch>-oci.tar.sha256
#   hodei-artifacts-api-<version>-<arch>-oci.digest
#   hodei-artifacts-api-<version>-<arch>.manifest.json
#   SHA256SUMS (correlating tarball, OCI archive, digest, and manifest)
#
# Usage: scripts/package-oci.sh [output-directory]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
BIN_NAME="hodei-artifacts-api"
MIGRATION_BIN_NAME="ensure-iam-index"
GRAPH_INDEX_BIN_NAME="ensure-graph-index"

cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
if [ -z "$VERSION" ]; then
  echo "error: could not read version from Cargo.toml" >&2
  exit 1
fi

ARCH="$(uname -m)"
COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
SHORT_COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}"
BUILD_DATE="$(date -u -d "@${SOURCE_DATE_EPOCH}" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"

TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "$TARGET_DIR" ]; then
  echo "error: could not read cargo target_directory" >&2
  exit 1
fi

PODMAN_STORAGE_DRIVER="${PODMAN_STORAGE_DRIVER:-vfs}"
PODMAN_ROOT="${PODMAN_ROOT:-/var/tmp/podman-root}"
PODMAN_RUNROOT="${PODMAN_RUNROOT:-/var/tmp/podman-runroot}"
PODMAN_CMD=(podman --storage-driver="$PODMAN_STORAGE_DRIVER" --root "$PODMAN_ROOT" --runroot "$PODMAN_RUNROOT")

# Build portable Linux binaries targeting Debian Bookworm glibc (2.36) with
# vendored OpenSSL so the runtime image needs no host-specific libssl.so.4.
ZIG_TARGET="${OCI_RUST_TARGET:-x86_64-unknown-linux-gnu.2.36}"
RUST_TRIPLE="${ZIG_TARGET%%.*}"

if command -v cargo-zigbuild >/dev/null 2>&1; then
  echo "==> building portable release binaries with cargo-zigbuild ($ZIG_TARGET)"
  cargo zigbuild --release --target "$ZIG_TARGET" --features vendored-openssl \
    --bin "$BIN_NAME" --bin "$MIGRATION_BIN_NAME" --bin "$GRAPH_INDEX_BIN_NAME"
  BIN_DIR="${TARGET_DIR}/${RUST_TRIPLE}/release"
else
  echo "==> building release binaries with cargo build"
  cargo build --release --features vendored-openssl \
    --bin "$BIN_NAME" --bin "$MIGRATION_BIN_NAME" --bin "$GRAPH_INDEX_BIN_NAME"
  BIN_DIR="${TARGET_DIR}/release"
fi

for bin in "$BIN_NAME" "$MIGRATION_BIN_NAME" "$GRAPH_INDEX_BIN_NAME"; do
  if [ ! -x "$BIN_DIR/$bin" ]; then
    echo "error: expected binary missing: $BIN_DIR/$bin" >&2
    exit 1
  fi
done

# Ensure the release tarball is also present in OUT_DIR for correlation.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
TARBALL_NAME="${BIN_NAME}-${VERSION}-${HOST_TRIPLE}.tar.gz"
if [ ! -f "$OUT_DIR/$TARBALL_NAME" ] || [ ! -f "$OUT_DIR/${TARBALL_NAME}.sha256" ]; then
  echo "==> building release tarball via scripts/package-release.sh"
  SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" "$ROOT/scripts/package-release.sh" "$OUT_DIR"
fi

# Stage deterministic OCI build context.
OCI_STAGE="$OUT_DIR/oci-stage"
rm -rf "$OCI_STAGE"
mkdir -p "$OCI_STAGE/bin" "$OCI_STAGE/config"

install -m 0755 "$BIN_DIR/$BIN_NAME" "$OCI_STAGE/bin/$BIN_NAME"
install -m 0755 "$BIN_DIR/$MIGRATION_BIN_NAME" "$OCI_STAGE/bin/$MIGRATION_BIN_NAME"
install -m 0755 "$BIN_DIR/$GRAPH_INDEX_BIN_NAME" "$OCI_STAGE/bin/$GRAPH_INDEX_BIN_NAME"

for cfg in default.toml release.toml standalone.toml external.toml; do
  if [ ! -f "$ROOT/config/$cfg" ]; then
    echo "error: missing config/$cfg" >&2
    exit 1
  fi
  install -m 0644 "$ROOT/config/$cfg" "$OCI_STAGE/config/$cfg"
done

find "$OCI_STAGE" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

IMAGE_TAG="localhost/hodei-artifacts-api:${VERSION}"
IMAGE_LATEST="localhost/hodei-artifacts-api:latest"

echo "==> building OCI image $IMAGE_TAG"
"${PODMAN_CMD[@]}" build \
  --format oci \
  --timestamp "$SOURCE_DATE_EPOCH" \
  --build-arg "VERSION=$VERSION" \
  --build-arg "VCS_REF=$SHORT_COMMIT" \
  --build-arg "BUILD_DATE=$BUILD_DATE" \
  -f "$ROOT/Containerfile" \
  -t "$IMAGE_TAG" \
  -t "$IMAGE_LATEST" \
  "$OCI_STAGE"

OCI_ARCHIVE_NAME="${BIN_NAME}-${VERSION}-${ARCH}-oci.tar"
OCI_ARCHIVE="$OUT_DIR/$OCI_ARCHIVE_NAME"
OCI_DIGEST_FILE="$OUT_DIR/${BIN_NAME}-${VERSION}-${ARCH}-oci.digest"
MANIFEST_FILE="$OUT_DIR/${BIN_NAME}-${VERSION}-${ARCH}.manifest.json"

rm -f "$OCI_ARCHIVE" "$OCI_ARCHIVE.sha256" "$OCI_DIGEST_FILE" "$MANIFEST_FILE"

echo "==> exporting OCI archive $OCI_ARCHIVE"
"${PODMAN_CMD[@]}" save --format oci-archive -o "$OCI_ARCHIVE" "$IMAGE_TAG"

IMAGE_DIGEST="$("${PODMAN_CMD[@]}" inspect --format '{{.Digest}}' "$IMAGE_TAG")"
printf '%s\n' "$IMAGE_DIGEST" > "$OCI_DIGEST_FILE"

(
  cd "$OUT_DIR"
  sha256sum "$OCI_ARCHIVE_NAME" > "${OCI_ARCHIVE_NAME}.sha256"
)

TARBALL_SHA256="$(cut -d' ' -f1 < "$OUT_DIR/${TARBALL_NAME}.sha256")"
OCI_ARCHIVE_SHA256="$(cut -d' ' -f1 < "$OUT_DIR/${OCI_ARCHIVE_NAME}.sha256")"

cat > "$MANIFEST_FILE" <<EOF
{
  "name": "${BIN_NAME}",
  "version": "${VERSION}",
  "commit": "${COMMIT}",
  "arch": "${ARCH}",
  "source_date_epoch": ${SOURCE_DATE_EPOCH},
  "tarball": {
    "filename": "${TARBALL_NAME}",
    "sha256": "${TARBALL_SHA256}"
  },
  "oci": {
    "image": "${IMAGE_TAG}",
    "digest": "${IMAGE_DIGEST}",
    "archive": "${OCI_ARCHIVE_NAME}",
    "sha256": "${OCI_ARCHIVE_SHA256}"
  }
}
EOF

(
  cd "$OUT_DIR"
  sha256sum \
    "$TARBALL_NAME" \
    "$OCI_ARCHIVE_NAME" \
    "$(basename "$OCI_DIGEST_FILE")" \
    "$(basename "$MANIFEST_FILE")" \
    > SHA256SUMS
)

echo
echo "oci-archive: $OCI_ARCHIVE"
echo "oci-digest:  $IMAGE_DIGEST"
echo "manifest:    $MANIFEST_FILE"
echo "checksums:   $OUT_DIR/SHA256SUMS"
