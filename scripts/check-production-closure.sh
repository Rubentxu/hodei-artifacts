#!/usr/bin/env bash
# Fails if the closure of the production tree links SurrealDB or RocksDB.
#
# HSP-213 retira el almacem embebido del binario productivo. El corte es una
# feature de Cargo (`legacy`), no una borrarra de codigo: las suites
# historicas y la herramienta de migracion siguen construyendo con la feature
# activada, y solo el arbol `--no-default-features` —el que empaqueta
# scripts/package-release.sh— ha de quedar limpio.
#
# Este script es la puerta de esa promesa: resuelve el arbol de dependencias
# de produccion y busca cualquier crate cuyo nombre mencione surreal o
# rocksdb. Si aparece alguno, el corte se ha roto y el script sale con error,
# nombrando la linea exacta.
#
# Usage: scripts/check-production-closure.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> resolviendo el arbol productivo (hodei-artifacts-api --no-default-features)"
tree="$(cargo tree -p hodei-artifacts-api --no-default-features -e no-dev 2>/dev/null)"

if printf '%s\n' "$tree" | grep -iqE 'surreal|rocksdb'; then
  echo "error: el cierre productivo aun enlaza el almacem embebido" >&2
  printf '%s\n' "$tree" | grep -iE 'surreal|rocksdb' >&2
  exit 1
fi

echo "OK: el arbol productivo no contiene surrealdb ni rocksdb"

# La contraprueba: con la feature `legacy` el arbol SI la trae. Si alguna vez
# esto falla, es que la feature dejo de tirar del almacem y el corte habria
# que rehacerse.
if ! cargo tree -p hodei-artifacts-api -e no-dev 2>/dev/null | grep -iqE 'surrealdb'; then
  echo "error: la feature 'legacy' ya no arranca surrealdb; el corte ha cambiado de forma" >&2
  exit 1
fi

echo "OK: la feature 'legacy' sigue trayendo el lector embebido (herramienta y suites)"
