#!/usr/bin/env bash
# TEST-ORG4: Verificación de ausencia de tests embebidos (`#[cfg(test)]` / `mod tests`) en src/**
#
# Objetivo:
#  - Hacer fail rápido en CI si se reintroducen tests inline contrariando la política
#    descrita en docs/testing-organization.md (Política "Cero Inline Tests").
#
# Uso:
#   bash scripts/verify-no-inline-tests.sh
#
# Exit codes:
#   0 -> OK (no hallazgos)
#   1 -> Hallazgos (violaciones)
#   2 -> Error de ejecución (misconfig / dependencias)
#
# Reglas detectadas:
#   - Aparición de patrón #[cfg(test)]
#   - Aparición de declaración de módulo exacta: mod tests {  (se permite espacio antes de '{')
#
# Allowlist:
#   - Variable INLINE_TEST_ALLOWLIST puede contener rutas (una por línea) que se ignorarán.
#   - Por defecto vacía (política estricta).
#
# Notas:
#   - Se ignoran archivos cuyo nombre termine en .disabled (histórico / archivado).
#   - Se muestran hasta MAX_CONTEXT líneas de contexto (grep -n simple).
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." &>/dev/null && pwd)"
cd "${ROOT_DIR}"

MAX_CONTEXT=0
ALLOWLIST_FILE="${INLINE_TEST_ALLOWLIST_FILE:-/dev/null}"

# Cargar allowlist en array (si existe y no es /dev/null)
declare -A ALLOW
if [[ -f "${ALLOWLIST_FILE}" ]]; then
  while IFS= read -r line; do
    [[ -z "${line}" || "${line}" =~ ^# ]] && continue
    # Normalizar a ruta relativa desde root
    ALLOW["${line}"]=1
  done < "${ALLOWLIST_FILE}"
fi

log() {
  printf '[verify-inline-tests] %s\n' "$*" >&2
}

# Recolectar ficheros fuente relevantes
mapfile -t FILES < <(find . -type f -path '*/src/*' -name '*.rs' ! -name '*.disabled' | sort)

if [[ ${#FILES[@]} -eq 0 ]]; then
  log "No se encontraron archivos .rs bajo src/** (¿estructura inesperada?)."
  exit 0
fi

violations=()

pattern_cfg='#[[:space:]]*cfg[[:space:]]*\(test\)'
pattern_mod='^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{'

for f in "${FILES[@]}"; do
  rel="${f#./}"
  [[ -n "${ALLOW[$rel]:-}" ]] && continue

  # Buscar patrones
  if grep -n -E "${pattern_cfg}" "${f}" >/dev/null || grep -n -E "${pattern_mod}" "${f}" >/dev/null; then
    # Extraer coincidencias detalladas
    while IFS= read -r line; do
      violations+=("${rel}:${line}")
    done < <(grep -n -E "${pattern_cfg}|${pattern_mod}" "${f}")
  fi
done

if [[ ${#violations[@]} -gt 0 ]]; then
  log "VIOLACIONES detectadas (${#violations[@]}). Política 'cero inline tests' incumplida:"
  printf '%s\n' "${violations[@]}" | sed 's/^/[hit] /'
  log "Revise docs/testing-organization.md y mueva los tests a tests/unit|it|e2e."
  log "Para excepciones formales, añadir a allowlist (actualmente desaconsejado)."
  exit 1
fi

log "OK: Sin tests embebidos detectados."
exit 0
