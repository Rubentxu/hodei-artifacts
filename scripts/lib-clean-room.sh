# shellcheck shell=bash
# Shared helper for the release tests (HSP-287): copy a served bundle into a
# brand-new, empty directory. Sourced, not executed.

# clean_room_copy <bundle-dir> <empty-destination>
# Copies only the regular files of the bundle (no stage/ directories, no build
# output), so the destination holds exactly what a consumer would download.
clean_room_copy() {
  local src="$1" dest="$2"
  [ ! -e "$dest" ] || { echo "error: clean room $dest already exists" >&2; return 1; }
  mkdir -p "$dest"
  find "$src" -maxdepth 1 -type f -exec cp {} "$dest"/ \;
}
