#!/usr/bin/env bash
# El gate de la UAT de doc-9.
#
# El informe de la UAT tiene que traer sus dieciseis escenarios, cada
# uno con su comando y su resultado. Este script lo comprueba, y falla
# con un motivo si no.
#
# Vive aqui y no en el `pipeline.kts` porque el DSL de la pipeline no
# admite variables de shell en los literales, y una comprobacion con
# variables es exactamente lo que hace falta.
set -euo pipefail

REPORT="${1:-target/uat/doc9.md}"

if [ ! -f "$REPORT" ]; then
    echo "the doc-9 report is missing: the suite did not write $REPORT" >&2
    exit 1
fi

# Los dieciseis escenarios de la UAT de doc-9: CAS-01..04, CAT-01..03,
# PUB-01..03 y RAW-01..06.
EXPECTED="CAS-01 CAS-02 CAS-03 CAS-04 CAT-01 CAT-02 CAT-03 PUB-01 PUB-02 PUB-03 RAW-01 RAW-02 RAW-03 RAW-04 RAW-05 RAW-06"

missing=""
for scenario in $EXPECTED; do
    if ! grep -q "^| $scenario |" "$REPORT"; then
        missing="$missing $scenario"
    fi
done

if [ -n "$missing" ]; then
    echo "the doc-9 report does not mention:$missing" >&2
    exit 1
fi

# Y ninguno puede aparecer dos veces: un escenario repetido es un
# escenario que se certifica dos veces, y eso no es evidencia.
count=$(grep -cE '^\| (CAS|CAT|PUB|RAW)-[0-9]+ \|' "$REPORT")
if [ "$count" -ne 16 ]; then
    echo "the doc-9 report has $count scenarios, and it must have 16" >&2
    exit 1
fi

# Y el informe tiene que decir que es lo que no certifica: un informe
# que solo dice lo que si pasa es propaganda.
if ! grep -q "no\*\* certifica" "$REPORT"; then
    echo "the doc-9 report does not say what it does not certify" >&2
    exit 1
fi

echo "the doc-9 report brings its 16 scenarios"
