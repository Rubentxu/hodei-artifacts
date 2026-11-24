#!/usr/bin/env bash
# Declara de forma visible si los tests de integracion contra Dgraph
# (`*_it`) van a ejecutarse de verdad. Cada target se salta a si mismo
# (exit 0) cuando HODEI_GRAPH_IT_ENDPOINT no esta definido; sin este
# aviso ese salto se leeria como un verde. Escribe el estado en
# target/uat/dgraph-it-status.txt para que quede como evidencia.
set -euo pipefail

mkdir -p target/uat
status=target/uat/dgraph-it-status.txt

if [ -z "${HODEI_GRAPH_IT_ENDPOINT:-}" ]; then
    {
        echo "NOT_RUN: HODEI_GRAPH_IT_ENDPOINT is not set."
        echo "The *_it targets of the 'integration (dgraph)' stage skip themselves"
        echo "and prove nothing. Start the Zero+Alpha (deploy/dgraph/compose.yaml)"
        echo "and export HODEI_GRAPH_IT_ENDPOINT to certify them."
    } | tee "$status" >&2
else
    echo "RUN: the *_it targets run against ${HODEI_GRAPH_IT_ENDPOINT}" | tee "$status"
fi
