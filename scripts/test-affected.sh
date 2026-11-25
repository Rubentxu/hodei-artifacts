#!/usr/bin/env bash
#
# Las pruebas de lo que ha cambiado.
#
# La pipeline de `pipeline.kts` es la PUERTA DE LA ENTREGA: ocho etapas
# sobre el workspace entero. Es la puerta que dice si el repositorio
# entero esta bien, y por eso no se puede abrir cada diez minutos.
#
# Este script es la puerta del dia a dia: mira lo que ha cambiado contra
# la rama base, deduce los paquetes afectados --y los que dependen de
# ellos-- y corre SOLO sus pruebas, mas el formato y los avisos de
# esos mismos paquetes.
#
# Que se corre lo decide `scripts/test-affected.py`: la decision necesita
# los metadatos del workspace --que fichero es de que paquete y quien
# depende de quien-- y eso es una pregunta de datos, no de shell. Aqui se
# decide COMO se corre, y se informa del resultado.
#
# Fallar es honesto: si la rama base no existe, si el diff no se puede
# resolver o si los metadatos no se pueden leer, el script falla con un
# motivo. Un gate que no se ha ejecutado es NOT_RUN, y un NOT_RUN nunca
# cuenta como PASS.
#
# El uso:
#
#   scripts/test-affected.sh [rama-base]
#
# Con la rama base por defecto `main`. Y con las opciones:
#
#   --no-clippy     no corre el formato ni los avisos (solo las pruebas)
#   --no-tests      no corre las pruebas (solo formato y avisos)
#   --openapi       corre el gate del contrato aunque no haya cambios
#   --plan          solo calcula y ensena el plan: no corre nada
#   --no-bounded    corre sin los limites de recursos de `bounded`
#   --report RUTA   donde escribe el informe JSON
#
# Salida: 0 si todo esta bien, 1 si algo falla.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PLANNER="${SCRIPT_DIR}/test-affected.py"

BASE_REF="main"
RUN_CLIPPY=1
RUN_TESTS=1
RUN_OPENAPI=0
PLAN_ONLY=0
USE_BOUNDED="auto"
REPORT_PATH=""

while [ $# -gt 0 ]; do
    case "$1" in
        --no-clippy) RUN_CLIPPY=0 ;;
        --no-tests) RUN_TESTS=0 ;;
        --openapi) RUN_OPENAPI=1 ;;
        --plan) PLAN_ONLY=1 ;;
        --no-bounded) USE_BOUNDED="0" ;;
        --report)
            shift
            REPORT_PATH="${1:-}"
            if [ -z "${REPORT_PATH}" ]; then
                echo "--report necesita una ruta" >&2
                exit 2
            fi
            ;;
        -h|--help)
            awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *) BASE_REF="$1" ;;
    esac
    shift
done

if [ ! -f "${PLANNER}" ]; then
    echo "falta el planificador ${PLANNER}: no se puede saber que hay que correr" >&2
    exit 1
fi

# ───────────────────────── el repositorio ─────────────────────────

if ! GIT_ROOT="$(git -C "${ROOT}" rev-parse --show-toplevel 2>/dev/null)"; then
    echo "este directorio no esta dentro de un repositorio git:" >&2
    echo "sin el no se puede saber que ha cambiado: no se ejecuta ninguna prueba" >&2
    exit 1
fi
cd "${GIT_ROOT}"

# ───────────────────────── lo que ha cambiado ─────────────────────────

# La rama base tiene que EXISTIR. Sin ella no hay diff, y sin diff no hay
# nada que probar: un script que no sabe que ha cambiado no puede decir que
# lo que ha cambiado esta bien.
if ! BASE_COMMIT="$(git rev-parse --verify --quiet "${BASE_REF}^{commit}")"; then
    echo "la rama base '${BASE_REF}' no existe en este repositorio" >&2
    echo "sin ella no se puede saber que ha cambiado: no se ejecuta ninguna prueba" >&2
    exit 1
fi

# Y tiene que haber un ancestro comun: sin el, el diff no significa nada.
if ! MERGE_BASE="$(git merge-base "${BASE_COMMIT}" HEAD)"; then
    echo "la rama base '${BASE_REF}' y HEAD no comparten ningun ancestro comun:" >&2
    echo "no se puede comparar lo que ha cambiado: no se ejecuta ninguna prueba" >&2
    exit 1
fi

if ! DIFF_FILES="$(git diff --name-only "${BASE_COMMIT}...HEAD")"; then
    echo "no se puede comparar HEAD contra '${BASE_REF}'" >&2
    echo "no se ejecuta ninguna prueba: un diff que no se resuelve no es un diff vacio" >&2
    exit 1
fi

# Los cambios que aun no estan committeados tambien son cambios: quien
# esta trabajando toca ficheros, no ramas.
WORKTREE_FILES="$(git status --porcelain --untracked-files=all | cut -c4- | sed 's/^.* -> //')"

FILES="$(printf '%s\n%s\n' "${DIFF_FILES}" "${WORKTREE_FILES}" | sed '/^$/d' | sort -u)"

HEAD_COMMIT="$(git rev-parse HEAD)"
BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo HEAD)"

# ───────────────────────── el plan ─────────────────────────

PLAN_DIR="${GIT_ROOT}/target/test-affected"
mkdir -p "${PLAN_DIR}/logs"

if [ -z "${REPORT_PATH}" ]; then
    REPORT_PATH="${PLAN_DIR}/report.json"
elif [ "${REPORT_PATH#/}" = "${REPORT_PATH}" ]; then
    REPORT_PATH="${GIT_ROOT}/${REPORT_PATH}"
fi

if ! printf '%s\n' "${FILES}" | python3 "${PLANNER}" plan \
        --root "${GIT_ROOT}" \
        --base-requested "${BASE_REF}" \
        --base-commit "${BASE_COMMIT}" \
        --merge-base "${MERGE_BASE}" \
        --commit "${HEAD_COMMIT}" \
        --branch "${BRANCH}" \
        --clippy "${RUN_CLIPPY}" \
        --fmt "${RUN_CLIPPY}" \
        --tests "${RUN_TESTS}" \
        --force-openapi "${RUN_OPENAPI}" \
        --out "${PLAN_DIR}/plan.json" \
        --tsv "${PLAN_DIR}/plan.tsv"; then
    echo "no se pudo calcular el plan de lo afectado: no se ejecuta ninguna prueba" >&2
    exit 1
fi

echo "── comparacion ──"
echo "  base: ${BASE_REF} -> ${BASE_COMMIT} (ancestro comun ${MERGE_BASE})"
echo "  commit: ${HEAD_COMMIT} (${BRANCH})"
echo "  ficheros cambiados: $(printf '%s\n' "${FILES}" | grep -c . || true)"

python3 "${PLANNER}" explain --plan "${PLAN_DIR}/plan.json"

# ───────────────────────── los limites ─────────────────────────

# `bounded` es un tope opcional de recursos: donde esta, se usa; donde no
# esta, se corre igual, pero se dice. Los limites no son la garantia: la
# garantia es que el gate se ha ejecutado.
if [ "${USE_BOUNDED}" = "auto" ]; then
    if command -v bounded >/dev/null 2>&1; then
        USE_BOUNDED=1
        LIMITS="bounded"
    else
        USE_BOUNDED=0
        LIMITS="none (bounded no esta instalado)"
    fi
elif [ "${USE_BOUNDED}" = "0" ]; then
    LIMITS="none (--no-bounded)"
else
    LIMITS="bounded"
fi

echo "── limites ──"
if [ "${USE_BOUNDED}" = "1" ]; then
    echo "  bounded: los gates se corren con el tope de recursos de la maquina"
else
    echo "  bounded no se usa: los gates se corren sin el tope de recursos (${LIMITS})"
fi

# ───────────────────────── la ejecucion ─────────────────────────

if [ "${PLAN_ONLY}" = "1" ]; then
    echo "── plan ──"
    echo "  no se ejecuta nada: es un plan, no un resultado"
    python3 "${PLANNER}" report \
        --plan "${PLAN_DIR}/plan.json" \
        --out "${REPORT_PATH}" \
        --mode plan \
        --limits "${LIMITS}"
    exit 0
fi

RESULTS="${PLAN_DIR}/results.tsv"
: > "${RESULTS}"

now_ms() {
    local value
    value="$(date +%s%3N 2>/dev/null || true)"
    case "${value}" in
        ''|*[!0-9]*) echo $(( $(date +%s) * 1000 )) ;;
        *) echo "${value}" ;;
    esac
}

record() {
    printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" >> "${RESULTS}"
}

run_gate() {
    local gate="$1" kind="$2" command="$3" reason="$4"
    local log="${PLAN_DIR}/logs/$(printf '%s' "${gate}" | tr ':' '_').log"
    local status exit_code note start end duration

    # Un gate que no se puede ejecutar NO es un gate que ha pasado: se
    # declara NOT_RUN, con el motivo, y el informe lo dirá.
    note=""
    case "${kind}" in
        openapi)
            if [ ! -f "${GIT_ROOT}/scripts/check-openapi.sh" ]; then
                record "${gate}" "NOT_RUN" 0 0 "falta scripts/check-openapi.sh"
                printf '  %-28s %-8s %s\n' "${gate}" "NOT_RUN" "falta scripts/check-openapi.sh"
                return
            fi
            ;;
        compose)
            if [ ! -f "${GIT_ROOT}/scripts/compose-smoke.sh" ]; then
                record "${gate}" "NOT_RUN" 0 0 "falta scripts/compose-smoke.sh"
                printf '  %-28s %-8s %s\n' "${gate}" "NOT_RUN" "falta scripts/compose-smoke.sh"
                return
            fi
            ;;
        frontend)
            if [ ! -f "${GIT_ROOT}/frontend/package.json" ]; then
                record "${gate}" "NOT_RUN" 0 0 "no hay frontend/package.json"
                printf '  %-28s %-8s %s\n' "${gate}" "NOT_RUN" "no hay frontend/package.json"
                return
            fi
            if [ ! -d "${GIT_ROOT}/frontend/node_modules" ]; then
                record "${gate}" "NOT_RUN" 0 0 "las dependencias del frontend no estan instaladas (npm ci)"
                printf '  %-28s %-8s %s\n' "${gate}" "NOT_RUN" "las dependencias del frontend no estan instaladas (npm ci)"
                return
            fi
            ;;
    esac

    echo "── ${gate} ──"
    echo "  porque ${reason}"
    echo "  ${command}"

    start="$(now_ms)"
    if [ "${USE_BOUNDED}" = "1" ]; then
        if [ "${command%% *}" = "cargo" ]; then
            bounded ${command} > "${log}" 2>&1 && exit_code=0 || exit_code=$?
        else
            bounded bash -c "${command}" > "${log}" 2>&1 && exit_code=0 || exit_code=$?
        fi
    else
        bash -c "${command}" > "${log}" 2>&1 && exit_code=0 || exit_code=$?
    fi
    end="$(now_ms)"
    duration=$(( end - start ))

    if [ "${exit_code}" -eq 0 ]; then
        status="PASS"
        note="log: ${log}"
    elif [ "${kind}" = "compose" ] && [ "${exit_code}" -eq 77 ]; then
        # El smoke devuelve 77 cuando no hay runtime, proveedor compose o
        # imagen: no ha verificado nada, y eso NO es un PASS (HSP-273).
        status="NOT_RUN"
        note="$(grep -m1 '^NOT_RUN:' "${log}" || echo "NOT_RUN: el smoke salio con 77")"
        note="${note}; log: ${log}"
    else
        status="FAIL"
        note="codigo de salida ${exit_code}; log: ${log}"
    fi

    record "${gate}" "${status}" "${duration}" "${exit_code}" "${note}"
    printf '  %-28s %-8s %s ms\n' "${gate}" "${status}" "${duration}"
    if [ "${status}" = "NOT_RUN" ]; then
        echo "  ${note}" >&2
    fi

    if [ "${status}" = "FAIL" ]; then
        tail -n 20 "${log}" >&2 || true
    fi
}

echo "── ejecucion ──"
while IFS=$'\t' read -r gate kind package command reason; do
    [ -n "${gate}" ] || continue
    run_gate "${gate}" "${kind}" "${command}" "${reason}"
done < "${PLAN_DIR}/plan.tsv"

python3 "${PLANNER}" report \
    --plan "${PLAN_DIR}/plan.json" \
    --results "${RESULTS}" \
    --out "${REPORT_PATH}" \
    --mode run \
    --limits "${LIMITS}"
