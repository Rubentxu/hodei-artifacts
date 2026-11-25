#!/usr/bin/env bash
# Builds the reproducible OCI container image, SBOMs (SPDX/CycloneDX),
# dependency/license matrix, build provenance attestation, and OpenPGP
# signatures anchored by a single release manifest.json.
#
# Produces in <output-directory> (default: ./dist):
#   hodei-artifacts-api-<version>-<target>.tar.gz
#   hodei-artifacts-api-<version>-<target>.tar.gz.sha256
#   hodei-artifacts-api-<version>-<arch>-oci.tar
#   hodei-artifacts-api-<version>-<arch>-oci.tar.sha256
#   hodei-artifacts-api-<version>-<arch>-oci.digest
#   hodei-artifacts-api-<version>-<arch>-oci.spdx.json
#   hodei-artifacts-api-<version>-<arch>-oci.cdx.json
#   hodei-artifacts-api-<version>-<arch>-cargo.spdx.json
#   hodei-artifacts-api-<version>-<arch>-licenses.json
#   hodei-artifacts-api-<version>-<arch>.provenance.json
#   hodei-artifacts-api-<version>-<arch>.manifest.json
#   hodei-artifacts-api-<version>-<arch>.manifest.json.asc
#   release-signing-key.asc
#   SHA256SUMS
#   SHA256SUMS.asc
#
# Usage: scripts/package-oci.sh [output-directory]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
BIN_NAME="hodei-artifacts-api"

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

run_syft() {
  if command -v syft >/dev/null 2>&1; then
    syft "$@"
  elif command -v mise >/dev/null 2>&1; then
    mise exec syft@latest -- syft "$@"
  else
    echo "error: syft not found in PATH and mise is unavailable" >&2
    exit 1
  fi
}

# Build portable Linux binary targeting Debian Bookworm glibc (2.36) with
# vendored OpenSSL so the runtime image needs no host-specific libssl.so.4.
# Only the production API binary is built and staged for the OCI image
# (no legacy migration binaries such as ensure-iam-index or ensure-graph-index).
ZIG_TARGET="${OCI_RUST_TARGET:-x86_64-unknown-linux-gnu.2.36}"
RUST_TRIPLE="${ZIG_TARGET%%.*}"

if command -v cargo-zigbuild >/dev/null 2>&1; then
  echo "==> building portable release binary with cargo-zigbuild ($ZIG_TARGET)"
  cargo zigbuild --release --target "$ZIG_TARGET" --features vendored-openssl --bin "$BIN_NAME"
  BIN_DIR="${TARGET_DIR}/${RUST_TRIPLE}/release"
  BUILD_RUST_TARGET="$ZIG_TARGET"
else
  echo "==> building release binary with cargo build"
  cargo build --release --features vendored-openssl --bin "$BIN_NAME"
  BIN_DIR="${TARGET_DIR}/release"
  BUILD_RUST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
fi

if [ ! -x "$BIN_DIR/$BIN_NAME" ]; then
  echo "error: expected binary missing: $BIN_DIR/$BIN_NAME" >&2
  exit 1
fi

# Ensure the release tarball is also present in OUT_DIR for correlation.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
TARBALL_NAME="${BIN_NAME}-${VERSION}-${HOST_TRIPLE}.tar.gz"
if [ ! -f "$OUT_DIR/$TARBALL_NAME" ] || [ ! -f "$OUT_DIR/${TARBALL_NAME}.sha256" ]; then
  echo "==> building release tarball via scripts/package-release.sh"
  SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" "$ROOT/scripts/package-release.sh" "$OUT_DIR"
fi

# Stage deterministic OCI build context containing ONLY hodei-artifacts-api + configs.
OCI_STAGE="$OUT_DIR/oci-stage"
rm -rf "$OCI_STAGE"
mkdir -p "$OCI_STAGE/bin" "$OCI_STAGE/config"

install -m 0755 "$BIN_DIR/$BIN_NAME" "$OCI_STAGE/bin/$BIN_NAME"
OCI_BIN_SHA256="$(sha256sum "$OCI_STAGE/bin/$BIN_NAME" | cut -d' ' -f1)"

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

PREFIX="${BIN_NAME}-${VERSION}-${ARCH}"
OCI_ARCHIVE_NAME="${PREFIX}-oci.tar"
OCI_ARCHIVE="$OUT_DIR/$OCI_ARCHIVE_NAME"
OCI_DIGEST_FILE="$OUT_DIR/${PREFIX}-oci.digest"
OCI_SPDX_NAME="${PREFIX}-oci.spdx.json"
OCI_SPDX_FILE="$OUT_DIR/$OCI_SPDX_NAME"
OCI_CDX_NAME="${PREFIX}-oci.cdx.json"
OCI_CDX_FILE="$OUT_DIR/$OCI_CDX_NAME"
CARGO_SPDX_NAME="${PREFIX}-cargo.spdx.json"
CARGO_SPDX_FILE="$OUT_DIR/$CARGO_SPDX_NAME"
LICENSE_MATRIX_NAME="${PREFIX}-licenses.json"
LICENSE_MATRIX_FILE="$OUT_DIR/$LICENSE_MATRIX_NAME"
PROVENANCE_NAME="${PREFIX}.provenance.json"
PROVENANCE_FILE="$OUT_DIR/$PROVENANCE_NAME"
MANIFEST_NAME="${PREFIX}.manifest.json"
MANIFEST_FILE="$OUT_DIR/$MANIFEST_NAME"
SIGNING_KEY_NAME="release-signing-key.asc"
SIGNING_KEY_FILE="$OUT_DIR/$SIGNING_KEY_NAME"

rm -f \
  "$OCI_ARCHIVE" "$OCI_ARCHIVE.sha256" "$OCI_DIGEST_FILE" \
  "$OCI_SPDX_FILE" "$OCI_CDX_FILE" "$CARGO_SPDX_FILE" \
  "$LICENSE_MATRIX_FILE" "$PROVENANCE_FILE" \
  "$MANIFEST_FILE" "${MANIFEST_FILE}.asc" \
  "$SIGNING_KEY_FILE" "$OUT_DIR/SHA256SUMS" "$OUT_DIR/SHA256SUMS.asc"

echo "==> exporting OCI archive $OCI_ARCHIVE"
"${PODMAN_CMD[@]}" save --format oci-archive -o "$OCI_ARCHIVE" "$IMAGE_TAG"

IMAGE_DIGEST="$("${PODMAN_CMD[@]}" inspect --format '{{.Digest}}' "$IMAGE_TAG")"
printf '%s\n' "$IMAGE_DIGEST" > "$OCI_DIGEST_FILE"

(
  cd "$OUT_DIR"
  sha256sum "$OCI_ARCHIVE_NAME" > "${OCI_ARCHIVE_NAME}.sha256"
)

echo "==> generating SPDX & CycloneDX SBOMs via syft"
run_syft scan "oci-archive:$OCI_ARCHIVE" -q -o "spdx-json=$OCI_SPDX_FILE"
run_syft scan "oci-archive:$OCI_ARCHIVE" -q -o "cyclonedx-json=$OCI_CDX_FILE"
run_syft scan "file:$ROOT/Cargo.lock" -q -o "spdx-json=$CARGO_SPDX_FILE"

echo "==> generating dependency & license matrix from cargo metadata"
CARGO_LOCK_SHA256="$(sha256sum "$ROOT/Cargo.lock" | cut -d' ' -f1)"
CARGO_METADATA_TMP="$OUT_DIR/.cargo-metadata.json"
cargo metadata --locked --format-version 1 > "$CARGO_METADATA_TMP"
python3 - "$CARGO_METADATA_TMP" "$LICENSE_MATRIX_FILE" "$ROOT/Cargo.toml" <<'PY'
import collections
import json
import sys

meta_path = sys.argv[1]
out_path = sys.argv[2]
cargo_toml_path = sys.argv[3]
with open(meta_path, "r", encoding="utf-8") as f:
    meta = json.load(f)

packages = []
license_counts = collections.Counter()
unlicensed = []
surrealdb_versions = []

for pkg in sorted(meta.get("packages", []), key=lambda p: (p["name"], p["version"])):
    lic = pkg.get("license") or pkg.get("license_file") or "UNKNOWN"
    if lic == "UNKNOWN":
        unlicensed.append(f"{pkg['name']}@{pkg['version']}")
    license_counts[lic] += 1
    if pkg["name"] == "surrealdb":
        surrealdb_versions.append(pkg["version"])
    packages.append({
        "name": pkg["name"],
        "version": pkg["version"],
        "license": lic,
        "source": pkg.get("source") or "workspace",
        "repository": pkg.get("repository"),
    })

with open(cargo_toml_path, "r", encoding="utf-8") as f:
    cargo_toml_text = f.read()
surrealdb_in_root_cargo = "surrealdb" in cargo_toml_text

matrix = {
    "schema_version": 1,
    "generator": "cargo metadata --locked --format-version 1",
    "total_packages": len(packages),
    "license_summary": dict(sorted(license_counts.items())),
    "unlicensed_packages": unlicensed,
    "surrealdb_status": {
        "present_in_cargo_toml": surrealdb_in_root_cargo,
        "present_in_resolved_graph": len(surrealdb_versions) > 0,
        "versions": surrealdb_versions,
        "note": (
            "surrealdb remains a direct Cargo.toml dependency pending HSP-213 removal; "
            "the production OCI image excludes legacy migration binaries."
            if surrealdb_in_root_cargo
            else "surrealdb excluded from Cargo.toml"
        ),
    },
    "packages": packages,
}

with open(out_path, "w", encoding="utf-8") as f:
    json.dump(matrix, f, indent=2)
    f.write("\n")
PY
rm -f "$CARGO_METADATA_TMP"

TARBALL_SHA256="$(cut -d' ' -f1 < "$OUT_DIR/${TARBALL_NAME}.sha256")"
OCI_ARCHIVE_SHA256="$(cut -d' ' -f1 < "$OUT_DIR/${OCI_ARCHIVE_NAME}.sha256")"
OCI_SPDX_SHA256="$(sha256sum "$OCI_SPDX_FILE" | cut -d' ' -f1)"
OCI_CDX_SHA256="$(sha256sum "$OCI_CDX_FILE" | cut -d' ' -f1)"
CARGO_SPDX_SHA256="$(sha256sum "$CARGO_SPDX_FILE" | cut -d' ' -f1)"
LICENSE_MATRIX_SHA256="$(sha256sum "$LICENSE_MATRIX_FILE" | cut -d' ' -f1)"

echo "==> generating build provenance attestation"
python3 - "$PROVENANCE_FILE" <<PY
import json
import sys

provenance = {
    "_type": "https://in-toto.io/Statement/v1",
    "subject": [
        {"name": "${TARBALL_NAME}", "digest": {"sha256": "${TARBALL_SHA256}"}},
        {"name": "${OCI_ARCHIVE_NAME}", "digest": {"sha256": "${OCI_ARCHIVE_SHA256}"}},
        {"name": "oci-binary:${BIN_NAME}", "digest": {"sha256": "${OCI_BIN_SHA256}"}},
        {"name": "${OCI_SPDX_NAME}", "digest": {"sha256": "${OCI_SPDX_SHA256}"}},
        {"name": "${OCI_CDX_NAME}", "digest": {"sha256": "${OCI_CDX_SHA256}"}},
        {"name": "${CARGO_SPDX_NAME}", "digest": {"sha256": "${CARGO_SPDX_SHA256}"}},
        {"name": "${LICENSE_MATRIX_NAME}", "digest": {"sha256": "${LICENSE_MATRIX_SHA256}"}},
    ],
    "predicateType": "https://slsa.dev/provenance/v1",
    "predicate": {
        "buildDefinition": {
            "buildType": "https://hodei.local/release/oci-and-tarball/v1",
            "externalParameters": {
                "version": "${VERSION}",
                "commit": "${COMMIT}",
                "arch": "${ARCH}",
                "source_date_epoch": int("${SOURCE_DATE_EPOCH}"),
                "rust_target": "${BUILD_RUST_TARGET}",
                "oci_image_digest": "${IMAGE_DIGEST}",
            },
            "resolvedDependencies": [
                {
                    "uri": "git+local@commit/${COMMIT}",
                    "digest": {"gitCommit": "${COMMIT}"},
                },
                {
                    "uri": "file:Cargo.lock",
                    "digest": {"sha256": "${CARGO_LOCK_SHA256}"},
                },
            ],
        },
        "runDetails": {
            "builder": {
                "id": "https://hodei.local/builders/local-workstation/v1",
            },
            "metadata": {
                "startedOn": "${BUILD_DATE}",
                "finishedOn": "${BUILD_DATE}",
                "honest_limits": (
                    "Local reproducible build attestation without hosted/isolated builder "
                    "non-falsifiability; does not claim SLSA Build Level 2 or higher."
                ),
            },
        },
    },
}

with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(provenance, f, indent=2)
    f.write("\n")
PY
PROVENANCE_SHA256="$(sha256sum "$PROVENANCE_FILE" | cut -d' ' -f1)"

# --- OpenPGP Signing (zero static CI secrets) -------------------------------
EPHEMERAL_GPG_DIR=""
cleanup_gpg() {
  if [ -n "$EPHEMERAL_GPG_DIR" ] && [ -d "$EPHEMERAL_GPG_DIR" ]; then
    rm -rf "$EPHEMERAL_GPG_DIR"
  fi
}
trap cleanup_gpg EXIT

if [ -n "${RELEASE_GPG_HOME:-}" ] && [ -n "${RELEASE_GPG_KEY_ID:-}" ]; then
  GPG_HOME="$RELEASE_GPG_HOME"
  GPG_KEY_ID="$RELEASE_GPG_KEY_ID"
  # scripts/release-bundle.sh hands its own (possibly ephemeral) key over through
  # RELEASE_GPG_*; it declares the real origin of that key so the manifest never
  # claims an operator keyring for a throwaway key.
  case "${RELEASE_SIGNING_KEY_MODE:-operator-keyring}" in
    operator-keyring|ephemeral-local) SIGNING_KEY_MODE="${RELEASE_SIGNING_KEY_MODE:-operator-keyring}" ;;
    *) echo "error: unsupported RELEASE_SIGNING_KEY_MODE: ${RELEASE_SIGNING_KEY_MODE}" >&2; exit 1 ;;
  esac
else
  EPHEMERAL_GPG_DIR="$(mktemp -d)"
  GPG_HOME="$EPHEMERAL_GPG_DIR/gpghome"
  mkdir -p "$GPG_HOME"
  chmod 700 "$GPG_HOME"
  cat > "$EPHEMERAL_GPG_DIR/keyparams" <<EOF
Key-Type: eddsa
Key-Curve: ed25519
Key-Usage: sign
Name-Real: Hodei Release Ephemeral Signer
Name-Email: release-ephemeral@hodei.local
Expire-Date: 0
%no-protection
%commit
EOF
  GNUPGHOME="$GPG_HOME" gpg --batch --gen-key "$EPHEMERAL_GPG_DIR/keyparams" >/dev/null 2>&1
  GPG_KEY_ID="$(GNUPGHOME="$GPG_HOME" gpg --with-colons --list-secret-keys | awk -F: '/^fpr:/ {print $10; exit}')"
  SIGNING_KEY_MODE="ephemeral-local"
fi

GNUPGHOME="$GPG_HOME" gpg --armor --export "$GPG_KEY_ID" > "$SIGNING_KEY_FILE"
SIGNING_KEY_SHA256="$(sha256sum "$SIGNING_KEY_FILE" | cut -d' ' -f1)"

SURREALDB_IN_CARGO="$(python3 -c "import json; print('true' if json.load(open('$LICENSE_MATRIX_FILE'))['surrealdb_status']['present_in_cargo_toml'] else 'false')")"

cat > "$MANIFEST_FILE" <<EOF
{
  "name": "${BIN_NAME}",
  "version": "${VERSION}",
  "commit": "${COMMIT}",
  "arch": "${ARCH}",
  "source_date_epoch": ${SOURCE_DATE_EPOCH},
  "build_date": "${BUILD_DATE}",
  "tarball": {
    "filename": "${TARBALL_NAME}",
    "sha256": "${TARBALL_SHA256}"
  },
  "oci": {
    "image": "${IMAGE_TAG}",
    "digest": "${IMAGE_DIGEST}",
    "archive": "${OCI_ARCHIVE_NAME}",
    "sha256": "${OCI_ARCHIVE_SHA256}",
    "binary_sha256": "${OCI_BIN_SHA256}",
    "legacy_migration_binaries_excluded": true,
    "surrealdb_in_cargo_toml": ${SURREALDB_IN_CARGO}
  },
  "sbom": {
    "oci_spdx": {
      "filename": "${OCI_SPDX_NAME}",
      "sha256": "${OCI_SPDX_SHA256}"
    },
    "oci_cyclonedx": {
      "filename": "${OCI_CDX_NAME}",
      "sha256": "${OCI_CDX_SHA256}"
    },
    "cargo_spdx": {
      "filename": "${CARGO_SPDX_NAME}",
      "sha256": "${CARGO_SPDX_SHA256}"
    }
  },
  "license_matrix": {
    "filename": "${LICENSE_MATRIX_NAME}",
    "sha256": "${LICENSE_MATRIX_SHA256}"
  },
  "provenance": {
    "filename": "${PROVENANCE_NAME}",
    "sha256": "${PROVENANCE_SHA256}"
  },
  "signing": {
    "mechanism": "openpgp-detached",
    "key_mode": "${SIGNING_KEY_MODE}",
    "public_key": "${SIGNING_KEY_NAME}",
    "public_key_sha256": "${SIGNING_KEY_SHA256}",
    "fingerprint": "${GPG_KEY_ID}",
    "signed_files": [
      "SHA256SUMS",
      "${MANIFEST_NAME}"
    ],
    "honest_limits": "Ephemeral OpenPGP Ed25519 key generated per build without static CI secrets; verifies bundle integrity and post-build tamper resistance, not external PKI/Rekor identity."
  }
}
EOF

(
  cd "$OUT_DIR"
  sha256sum \
    "$TARBALL_NAME" \
    "$OCI_ARCHIVE_NAME" \
    "$(basename "$OCI_DIGEST_FILE")" \
    "$OCI_SPDX_NAME" \
    "$OCI_CDX_NAME" \
    "$CARGO_SPDX_NAME" \
    "$LICENSE_MATRIX_NAME" \
    "$PROVENANCE_NAME" \
    "$MANIFEST_NAME" \
    "$SIGNING_KEY_NAME" \
    > SHA256SUMS
)

echo "==> signing SHA256SUMS and $MANIFEST_NAME with OpenPGP ($SIGNING_KEY_MODE)"
GNUPGHOME="$GPG_HOME" gpg --batch --yes --armor --detach-sign -u "$GPG_KEY_ID" -o "$OUT_DIR/SHA256SUMS.asc" "$OUT_DIR/SHA256SUMS"
GNUPGHOME="$GPG_HOME" gpg --batch --yes --armor --detach-sign -u "$GPG_KEY_ID" -o "${MANIFEST_FILE}.asc" "$MANIFEST_FILE"

echo
echo "oci-archive:    $OCI_ARCHIVE"
echo "oci-digest:     $IMAGE_DIGEST"
echo "sbom-oci-spdx:  $OCI_SPDX_FILE"
echo "sbom-oci-cdx:   $OCI_CDX_FILE"
echo "sbom-cargo:     $CARGO_SPDX_FILE"
echo "license-matrix: $LICENSE_MATRIX_FILE"
echo "provenance:     $PROVENANCE_FILE"
echo "manifest:       $MANIFEST_FILE"
echo "checksums:      $OUT_DIR/SHA256SUMS"
echo "signatures:     $OUT_DIR/SHA256SUMS.asc, ${MANIFEST_FILE}.asc"
