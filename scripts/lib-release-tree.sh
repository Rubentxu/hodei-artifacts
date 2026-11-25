#!/usr/bin/env bash
# Shared by scripts/package-oci.sh and scripts/release-bundle.sh (HSP-289).
#
# `release_tree_clean <root> [<output-dir>]` prints `true` when the git working
# tree (tracked changes and untracked, non-ignored files, ignoring the output
# directory) has no modifications, `false` otherwise. A directory that is not a
# git work tree is never clean: provenance cannot vouch for it.
#
# `release_require_clean_tree <root> [<output-dir>]` fails with a clear error on
# a dirty tree unless HODEI_ALLOW_DIRTY_TREE=1, in which case it still reports
# `false` so provenance records `tree_clean: false` instead of a silent claim.

release_tree_clean() {
  local root="$1" out="${2:-}" rel="" status
  root="$(realpath -m "$root")"
  [ -z "$out" ] || out="$(realpath -m "$out")"
  git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo false; return 0; }
  local args=(status --porcelain)
  case "$out" in
    "$root"/*) rel="${out#"$root"/}"; args+=(-- . ":(exclude)$rel") ;;
  esac
  status="$(git -C "$root" "${args[@]}" 2>/dev/null)" || { echo false; return 0; }
  if [ -z "$status" ]; then echo true; else echo false; fi
}

release_require_clean_tree() {
  local clean
  clean="$(release_tree_clean "$@")"
  if [ "$clean" != "true" ] && [ "${HODEI_ALLOW_DIRTY_TREE:-0}" != "1" ]; then
    echo "error: the working tree is dirty (or not a git checkout): a release must be built from committed sources." >&2
    echo "       Commit or stash the changes, or set HODEI_ALLOW_DIRTY_TREE=1 to record tree_clean=false knowingly." >&2
    return 1
  fi
  echo "$clean"
}
