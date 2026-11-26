#!/usr/bin/env bash
# El gate del contrato OpenAPI (HSP-130).
#
# Tres cosas, en este orden:
#
# 1. LINT: el contrato modular valida limpio con la configuración que
#    vive en redocly.yaml (la misma para quien edita y para el gate).
# 2. BUNDLE: el contrato se empaqueta a un único fichero. Es lo que un
#    consumidor (un SDK, un explorador de API, un cliente HTTP) se lleva.
# 3. DERIVA: el bundle recién generado se compara con el que está
#    versionado (docs/openapi/bundled.yaml). Si difieren, alguien editó
#    el contrato modular sin regenerar el bundle, o el bundle a mano:
#    las dos fuentes de verdad divergen y el gate lo dice.
#
# Códigos de salida:
#   0  limpio, bundle en sincronía
#   1  el lint falla
#   2  el bundle no se puede generar
#   3  deriva: el bundle versionado no coincide con el generado
#
# La publicación del contrato NO vive aquí: el bundle versionado es el
# entregable, y publicarlo fuera es una decisión de la persona, no del
# gate (la CI es local y no publica nada).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
SRC="$ROOT/docs/openapi/openapi.yaml"
BUNDLED="$ROOT/docs/openapi/bundled.yaml"
OUT="$ROOT/target/openapi/bundled.generated.yaml"

# The output directory lives under target/, which a clean checkout lacks.
mkdir -p "$ROOT/target/openapi"

# 1. Lint: limpio significa cero errores y cero avisos. La configuración
#    de las reglas vive en redocly.yaml, con sus excepciones escritas.
if ! npx -y @redocly/cli lint "$SRC" > "$ROOT/target/openapi/lint.log" 2>&1; then
    echo "the OpenAPI contract does not validate clean (see target/openapi/lint.log):" >&2
    grep -E "^\[[0-9]+\]|Validation failed" "$ROOT/target/openapi/lint.log" | head -10 >&2
    exit 1
fi
if grep -qE "[0-9]+ (error|warning)" "$ROOT/target/openapi/lint.log" \
   && ! grep -qE "0 errors and 0 warnings|is valid" "$ROOT/target/openapi/lint.log"; then
    echo "the OpenAPI contract validates with warnings (see target/openapi/lint.log)" >&2
    exit 1
fi

# 2. Bundle: el contrato entero, en un fichero.
if ! npx -y @redocly/cli bundle "$SRC" -o "$OUT" > "$ROOT/target/openapi/bundle.log" 2>&1; then
    echo "the OpenAPI contract cannot be bundled (see target/openapi/bundle.log)" >&2
    exit 2
fi

# 3. Deriva: el bundle versionado contra el recién generado.
if [ ! -f "$BUNDLED" ]; then
    echo "docs/openapi/bundled.yaml is missing: commit the bundle the gate generates" >&2
    exit 3
fi
if ! diff -q "$BUNDLED" "$OUT" > /dev/null 2>&1; then
    echo "the bundled contract drifted from the modular source:" >&2
    echo "  regenerate it with: npx -y @redocly/cli bundle docs/openapi/openapi.yaml -o docs/openapi/bundled.yaml" >&2
    diff "$BUNDLED" "$OUT" | head -20 >&2
    exit 3
fi

echo "the OpenAPI contract validates clean and the bundle is in sync"
