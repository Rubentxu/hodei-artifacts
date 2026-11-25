#!/usr/bin/env bash
# Builds the release distribution of the `hodei-artifacts-api` binary.
#
# # What it produces
#
#   <out>/hodei-artifacts-api-<version>-<target>.tar.gz
#   <out>/hodei-artifacts-api-<version>-<target>.tar.gz.sha256
#
# The package carries the community quickstart with it: the tutorial, the
# script that executes it and the templates of the laboratory. Somebody who
# downloads the distribution can run it without a toolchain.
#
# # Why it is a script and not a shell history
#
# The release asset has to be regenerable from a commit: the same script, at
# the same commit, has to produce the same layout. Assembling the tarball by
# hand is how a distribution ends up shipping a file the program never reads.
#
# # The configuration file is not optional
#
# The service deserializes its configuration strictly and reads
# `config/default.toml` relative to the working directory. A distribution
# without that file does not start ("missing field `db_type`"), so the script
# refuses to build a package that does not contain it.
#
# # The build profile
#
# The profile is `release` unless `HODEI_PACKAGE_PROFILE=debug` is set. The
# debug profile exists so that the test suites can package the very layout
# of the distribution (this script, the same files) without paying for an
# optimized build. A debug package is never a release asset.
#
# Usage: scripts/package-release.sh [output-directory]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
BIN_NAME="hodei-artifacts-api"
# The operator command that declares the account index of the IAM policies.
# It travels with the service because the deployment needs it: the index
# cannot be declared by the running service, and a deployment that cannot
# run it would keep scanning the table.
MIGRATION_BIN_NAME="ensure-iam-index"
# The operator command that declares the indexes of the evidence graph. It
# travels with the service for the same reason: the indexes cannot be
# declared by the running service, and the measure of the scale model
# showed what the graph costs without them.
GRAPH_INDEX_BIN_NAME="ensure-graph-index"

PROFILE="${HODEI_PACKAGE_PROFILE:-release}"
case "$PROFILE" in
  release) PROFILE_FLAGS=(--release) ;;
  debug) PROFILE_FLAGS=() ;;
  *)
    echo "error: HODEI_PACKAGE_PROFILE must be 'release' or 'debug', not '$PROFILE'" >&2
    exit 1
    ;;
esac

cd "$ROOT"

# --- Identity ---------------------------------------------------------------

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
if [ -z "$VERSION" ]; then
  echo "error: could not read the version from Cargo.toml" >&2
  exit 1
fi

TARGET="$(rustc -vV | sed -n 's/^host: //p')"
if [ -z "$TARGET" ]; then
  echo "error: could not read the host target from rustc" >&2
  exit 1
fi

# The target directory is configured outside the repository on some hosts, so
# it is asked for instead of assumed.
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "$TARGET_DIR" ]; then
  echo "error: could not read the cargo target directory" >&2
  exit 1
fi

PKG_NAME="${BIN_NAME}-${VERSION}-${TARGET}"
STAGE="$OUT_DIR/stage/$PKG_NAME"
ARCHIVE="$OUT_DIR/${PKG_NAME}.tar.gz"

# --- Preflight --------------------------------------------------------------

CONFIG_FILE="config/default.toml"
if [ ! -f "$CONFIG_FILE" ]; then
  echo "error: $CONFIG_FILE is missing; the distribution would not boot" >&2
  exit 1
fi

for required in README.md LICENSE CHANGELOG.md; do
  if [ ! -f "$required" ]; then
    echo "error: $required is missing; the distribution would be incomplete" >&2
    exit 1
  fi
done

# The community path travels with the distribution, so the files it needs
# are checked here too: a package without them would download and then not
# run, which is the worst way to fail.
for required in \
  scripts/community-quickstart.sh \
  config/community/schema.json \
  config/community/policies.cedar \
  config/release.toml \
  docs/community/quickstart.md; do
  if [ ! -f "$required" ]; then
    echo "error: $required is missing; the community quickstart would not run" >&2
    exit 1
  fi
done

# --- Build ------------------------------------------------------------------

echo "==> building the $PROFILE binaries"
cargo build ${PROFILE_FLAGS[@]+"${PROFILE_FLAGS[@]}"} --bin "$BIN_NAME" --bin "$MIGRATION_BIN_NAME" --bin "$GRAPH_INDEX_BIN_NAME"

BIN_PATH="$(find "${TARGET_DIR}/${PROFILE}" -maxdepth 1 -type f -name "$BIN_NAME" -perm -u+x | head -n 1)"
if [ -z "$BIN_PATH" ]; then
  echo "error: the release binary was not produced" >&2
  exit 1
fi

MIGRATION_BIN_PATH="$(find "${TARGET_DIR}/${PROFILE}" -maxdepth 1 -type f -name "$MIGRATION_BIN_NAME" -perm -u+x | head -n 1)"
if [ -z "$MIGRATION_BIN_PATH" ]; then
  echo "error: the operator command was not produced" >&2
  exit 1
fi

GRAPH_INDEX_BIN_PATH="$(find "${TARGET_DIR}/${PROFILE}" -maxdepth 1 -type f -name "$GRAPH_INDEX_BIN_NAME" -perm -u+x | head -n 1)"
if [ -z "$GRAPH_INDEX_BIN_PATH" ]; then
  echo "error: the operator command of the graph indexes was not produced" >&2
  exit 1
fi

# --- Assemble ---------------------------------------------------------------

echo "==> assembling $PKG_NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/config" "$STAGE/config/community"

install -m 0755 "$BIN_PATH" "$STAGE/bin/$BIN_NAME"
install -m 0755 "$MIGRATION_BIN_PATH" "$STAGE/bin/$MIGRATION_BIN_NAME"
install -m 0755 "$GRAPH_INDEX_BIN_PATH" "$STAGE/bin/$GRAPH_INDEX_BIN_NAME"
install -m 0644 "$CONFIG_FILE" "$STAGE/config/default.toml"

# The community quickstart travels with the distribution.
#
# The point of the distribution is that somebody can run it without a
# toolchain: the tutorial, the script that executes it and the templates
# of the laboratory have to be in the package, because the person who
# downloads it has nothing else. The production profile travels too, for
# the same reason: it is the profile an operator deploys, and it is the
# one the guard of the quickstart checks.
install -m 0644 config/release.toml "$STAGE/config/release.toml"
install -m 0644 config/community/schema.json "$STAGE/config/community/schema.json"
install -m 0644 config/community/policies.cedar "$STAGE/config/community/policies.cedar"
install -m 0755 scripts/community-quickstart.sh "$STAGE/community-quickstart.sh"
install -m 0644 docs/community/quickstart.md "$STAGE/QUICKSTART.md"

install -m 0644 README.md "$STAGE/README.md"
install -m 0644 LICENSE "$STAGE/LICENSE"
install -m 0644 CHANGELOG.md "$STAGE/CHANGELOG.md"

# --- Archive ----------------------------------------------------------------

echo "==> archiving"
mkdir -p "$OUT_DIR"
rm -f "$ARCHIVE" "$ARCHIVE.sha256"

# Deterministic metadata: sorted entries, a fixed timestamp and no owner
# information, so two runs of this script at the same commit differ only in
# the bytes of the compiled binary itself.
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}"

tar \
  --sort=name \
  --mtime="@${SOURCE_DATE_EPOCH}" \
  --owner=0 --group=0 --numeric-owner \
  --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
  -C "$OUT_DIR/stage" -czf "$ARCHIVE" "$PKG_NAME"

( cd "$OUT_DIR" && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256" )

echo
echo "distribution: $ARCHIVE"
echo "sha256:      $(cut -d' ' -f1 < "$ARCHIVE.sha256")"
