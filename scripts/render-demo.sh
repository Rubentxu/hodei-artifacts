#!/usr/bin/env bash
#
# Records the community quickstart for real and renders the visual demo that
# the READMEs link.
#
# # What it produces
#
#   docs/community/quickstart-demo.cast   the asciicast of a REAL run (the source)
#   docs/community/quickstart-demo.gif    the animation GitHub can display
#
# A .cast file is not rendered by GitHub, so the GIF is the linked asset and
# the cast is kept next to it as the auditable recording it came from.
#
# # Where the frames come from
#
# Nothing is typed by hand or faked: `asciinema rec` runs
# `scripts/community-quickstart.sh run` against a distribution, so the frames
# show the real service starting, the real token being minted, the Raw and
# Maven publishes, the pulls with their digests, the restart and the 401 an
# anonymous caller gets. The console trust view is NOT part of that flow and
# is not shown.
#
# # Usage
#
#   scripts/render-demo.sh --archive dist/hodei-artifacts-api-<version>-<target>.tar.gz
#   scripts/render-demo.sh --distribution path/to/extracted/distribution
#   scripts/render-demo.sh --binary target/release/hodei-artifacts-api
#   scripts/render-demo.sh --render-only      # re-render the GIF from the committed cast
#
# Options: --out-dir DIR (default docs/community), --cols N, --rows N.
#
# # Requirements
#
# `asciinema` (>= 3) to record and `agg` to render the GIF. Neither needs the
# network. Digests, ports and the lab path differ on every recording.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

OUT_DIR="$REPO_DIR/docs/community"
COLS=112
ROWS=50
ARCHIVE=""
DISTRIBUTION=""
BINARY=""
RENDER_ONLY=0

die() { echo "error: $*" >&2; exit 1; }

while [ "$#" -gt 0 ]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --distribution) DISTRIBUTION="$2"; shift 2 ;;
    --binary) BINARY="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --cols) COLS="$2"; shift 2 ;;
    --rows) ROWS="$2"; shift 2 ;;
    --render-only) RENDER_ONLY=1; shift ;;
    -h|--help) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d'; exit 0 ;;
    *) die "unknown option '$1'" ;;
  esac
done

CAST="$OUT_DIR/quickstart-demo.cast"
GIF="$OUT_DIR/quickstart-demo.gif"

command -v agg >/dev/null 2>&1 || die "'agg' is needed to render the GIF"

if [ "$RENDER_ONLY" -eq 0 ]; then
  command -v asciinema >/dev/null 2>&1 || die "'asciinema' is needed to record the run"
  mkdir -p "$OUT_DIR"

  WORK="$(mktemp -d /tmp/hodei-demo.XXXXXX)"
  trap 'rm -rf "$WORK"' EXIT

  RUN_ARGS=()
  if [ -n "$ARCHIVE" ]; then
    RUN_ARGS=(--archive "$ARCHIVE")
  elif [ -n "$DISTRIBUTION" ]; then
    RUN_ARGS=(--distribution "$DISTRIBUTION")
  elif [ -n "$BINARY" ]; then
    # Stage the layout the quickstart expects from a binary already built.
    [ -x "$BINARY" ] || die "the binary '$BINARY' is not executable"
    mkdir -p "$WORK/dist/bin" "$WORK/dist/config"
    install -m 0755 "$BINARY" "$WORK/dist/bin/hodei-artifacts-api"
    install -m 0644 "$REPO_DIR/config/default.toml" "$WORK/dist/config/default.toml"
    RUN_ARGS=(--distribution "$WORK/dist")
  else
    die "pass --archive, --distribution or --binary (or --render-only)"
  fi

  rm -f "$CAST"
  asciinema rec --headless --quiet --return \
    --window-size "${COLS}x${ROWS}" \
    --title "hodei-artifacts community quickstart" \
    --command "$REPO_DIR/scripts/community-quickstart.sh run ${RUN_ARGS[*]@Q}" \
    "$CAST"
fi

[ -f "$CAST" ] || die "the cast '$CAST' does not exist"
agg --cols "$COLS" --rows "$ROWS" --idle-time-limit 1.5 --last-frame-duration 6 \
  "$CAST" "$GIF"
echo "rendered: $GIF ($(wc -c < "$GIF") bytes) from $CAST"
