#!/usr/bin/env bash
# Fails if the dependency tree links SurrealDB or RocksDB.
#
# HSP-213 retired the embedded store from the production binary and
# HSP-314 F4 deleted it: there is no `legacy` feature and no embedded engine
# left, so the promise is now unconditional. This script resolves the
# dependency tree of the whole workspace, production and dev edges, with every
# feature enabled, and looks for any crate whose name mentions surreal or
# rocksdb (librocksdb-sys included). A hit names the exact line.
#
# Usage: scripts/check-production-closure.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

check_tree() {
  local label="$1"
  shift
  echo "==> resolviendo el arbol: $label"
  local tree
  tree="$(cargo tree "$@" 2>/dev/null)"
  if [ -z "$tree" ]; then
    echo "error: cargo tree no devolvio nada para: $label" >&2
    exit 1
  fi
  if printf '%s\n' "$tree" | grep -iqE 'surreal|rocksdb'; then
    echo "error: el arbol ($label) enlaza el almacen embebido retirado" >&2
    printf '%s\n' "$tree" | grep -iE 'surreal|rocksdb' >&2
    exit 1
  fi
  echo "OK: el arbol ($label) no contiene surrealdb ni rocksdb"
}

check_tree "productivo, sin features por defecto" \
  -p hodei-artifacts-api --no-default-features -e no-dev
check_tree "por defecto" -p hodei-artifacts-api -e no-dev
check_tree "workspace completo, todas las features y dependencias de desarrollo" \
  --workspace --all-features -e all
