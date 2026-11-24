#!/usr/bin/env bash
# Generates a Homebrew formula from one or more release manifests.
#
# Each manifest (`<name>-<version>-<arch>.manifest.json`, produced by
# scripts/package-oci.sh) names the tarball and its SHA-256. The target triple
# is read from the tarball filename. Pass one manifest per platform to get a
# multi-platform formula; all manifests must share the same version.
#
# Usage: scripts/generate-homebrew-formula.sh [--base-url URL] <manifest.json>...
#
# The formula is printed on stdout. Commit it to the tap repository.

set -euo pipefail

BASE_URL=""
MANIFESTS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --base-url) BASE_URL="${2:?--base-url needs a value}"; shift 2 ;;
    -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) MANIFESTS+=("$1"); shift ;;
  esac
done
if [ "${#MANIFESTS[@]}" -eq 0 ]; then
  echo "usage: $0 [--base-url URL] <manifest.json>..." >&2
  exit 1
fi

exec python3 -I - "$BASE_URL" "${MANIFESTS[@]}" <<'PY'
import json
import re
import sys

base_url, paths = sys.argv[1], sys.argv[2:]
REPO = "https://github.com/Rubentxu/hodei-artifacts"
PLATFORMS = {
    "x86_64-unknown-linux-gnu": ("on_linux", "on_intel"),
    "aarch64-unknown-linux-gnu": ("on_linux", "on_arm"),
    "x86_64-apple-darwin": ("on_macos", "on_intel"),
    "aarch64-apple-darwin": ("on_macos", "on_arm"),
}

entries = {}
version = name = None
for path in paths:
    with open(path, encoding="utf-8") as f:
        m = json.load(f)
    for key in ("name", "version", "tarball"):
        if not m.get(key):
            raise SystemExit(f"{path}: manifest missing field: {key}")
    if version not in (None, m["version"]):
        raise SystemExit("manifests carry different versions")
    version, name = m["version"], m["name"]
    fname, sha = m["tarball"]["filename"], m["tarball"]["sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", sha):
        raise SystemExit(f"{path}: tarball sha256 is not a valid digest")
    match = re.fullmatch(re.escape(f"{name}-{version}-") + r"(.+)\.tar\.gz", fname)
    if not match or match.group(1) not in PLATFORMS:
        raise SystemExit(f"{path}: tarball {fname!r} has no supported target triple")
    entries[match.group(1)] = (fname, sha)

root = base_url.rstrip("/") if base_url else f"{REPO}/releases/download/v{version}"
cls = "".join(p.capitalize() for p in re.split(r"[-_]", name))

out = [
    f"class {cls} < Formula",
    '  desc "Hodei Artifacts: graph-first artifact registry with Cedar authorization"',
    f'  homepage "{REPO}"',
    f'  version "{version}"',
    '  license "MIT"',
    "",
]
for os_block in ("on_macos", "on_linux"):
    arches = [(PLATFORMS[t][1], t) for t in sorted(entries) if PLATFORMS[t][0] == os_block]
    if not arches:
        continue
    out.append(f"  {os_block} do")
    for arch, triple in sorted(arches):
        fname, sha = entries[triple]
        out += [f"    {arch} do",
                f'      url "{root}/{fname}"',
                f'      sha256 "{sha}"',
                "    end"]
    out += ["  end", ""]

out += [
    "  def install",
    # Homebrew strips the single top-level directory of the tarball.
    '    bin.install Dir["bin/*"]',
    '    pkgshare.install "config"',
    "  end",
    "",
    "  test do",
    f'    assert_predicate bin/"{name}", :executable?',
    "  end",
    "end",
]
print("\n".join(out))
PY
