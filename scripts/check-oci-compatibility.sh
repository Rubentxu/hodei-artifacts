#!/usr/bin/env bash
# El informe de la compatibilidad OCI tiene que estar **completo**.
#
# La etapa de la CI archiva el informe y falla si **falta**, pero un
# informe que existe con menos casos--porque un caso se renombro, se marco
# como ignorado o se borro-- es el informe de una suite que no es la que
# la etapa cree. Aqui no se ejecuta nada: se lee lo que la suite escribio
# y se dice lo que no esta.
#
# Codigos de salida:
#   0  el informe tiene todos los casos y ninguno en rojo
#   1  falta un caso, o hay uno en rojo
set -euo pipefail

REPORT="${1:-target/uat/oci-compatibility.md}"

# Los casos que la suite escribe. La lista es la de los casos del binario
# de pruebas, y un caso que se anade aqui sin anadirlo alla--o al
# reves--lo dice este script.
EXPECTED=(
    "OCI-PUSH-03 (podman)"
    "OCI-PUSH-03 (docker, una plataforma)"
    "OCI-INDEX-01 (docker, multi-plataforma)"
    "OCI-REFERRERS-01 (oras)"
    "OCI-ISOLATION-01 (podman, otra cuenta)"
    "OCI-RETRY-01 (podman, reintento)"
    "OCI-DURABILITY-01 (podman, reinicio)"
    "OCI-HIERARCHY-01 (podman, niveles)"
    "OCI-CONFORM-01 (suite oficial)"
)

if [ ! -f "$REPORT" ]; then
    echo "the OCI compatibility report is missing: $REPORT" >&2
    exit 1
fi

missing=0
for case in "${EXPECTED[@]}"; do
    if ! grep -Fq "| $case |" "$REPORT"; then
        echo "the report does not have the case '$case'" >&2
        missing=1
    fi
done

if grep -Fq "| FALLO" "$REPORT"; then
    echo "the report has a case in red" >&2
    missing=1
fi

exit "$missing"
