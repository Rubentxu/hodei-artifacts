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
# El cambio de un crate de dominio arrastra a quien depende de el, y por
# eso la deduplicacion es por DEPENDENCIAS y no por rutas: cambiar el
# nucleo toca el kernel, la API y todo lo que esta encima.
#
# El uso:
#
#   scripts/test-affected.sh [rama-base]
#
# Con la rama base por defecto `main`. Y con las opciones:
#
#   --no-clippy     no corre los avisos (solo las pruebas)
#   --no-tests      no corre las pruebas (solo formato y avisos)
#   --openapi       corre el gate del contrato aunque no haya cambios
#
# Salida: 0 si todo esta bien, 1 si algo falla.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BASE_REF="main"
RUN_CLIPPY=1
RUN_TESTS=1
RUN_OPENAPI=0

while [ $# -gt 0 ]; do
    case "$1" in
        --no-clippy) RUN_CLIPPY=0 ;;
        --no-tests) RUN_TESTS=0 ;;
        --openapi) RUN_OPENAPI=1 ;;
        -h|--help)
            awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *) BASE_REF="$1" ;;
    esac
    shift
done

# ───────────────────────── lo que ha cambiado ─────────────────────────

changed_files() {
    {
        git diff --name-only "${BASE_REF}...HEAD" 2>/dev/null || true
        git status --porcelain 2>/dev/null | awk '{print $NF}' || true
    } | sed '/^$/d' | sort -u
}

FILES="$(changed_files)"
if [ -z "${FILES}" ]; then
    echo "no hay cambios contra ${BASE_REF}: no hay nada que probar"
    exit 0
fi

# ───────────────────────── los paquetes afectados ─────────────────────────

# El mapeo lo hace el manifiesto del workspace y no un recorrido de
# rutas: el `cargo metadata` sabe que archivos son de que paquete, y
# ademas dice quien depende de quien. Un cambio en un crate de dominio
# arrastra a quien depende de el, y si no se arrastra el fallo aparece en
# el sitio equivocado.
mapfile -t FILE_LIST <<< "${FILES}"

PACKAGES="$(python3 - "${FILE_LIST[@]}" <<'PYTHON'
import json
import os
import subprocess
import sys

files = [line.strip() for line in sys.argv[1:] if line.strip()]
# El manifiesto dice las rutas en absoluto y el cambio las da en
# relativo: los dos lados tienen que hablar el mismo idioma.
files = [os.path.abspath(path) for path in files]

metadata = subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"],
    check=True,
    capture_output=True,
    text=True,
).stdout
packages = json.loads(metadata)["packages"]

# Los archivos de cada paquete: los de sus targets y su manifiesto.
sources = {}
dependencies = {}
for package in packages:
    paths = [target["src_path"] for target in package["targets"]]
    paths.append(package["manifest_path"])
    sources[package["name"]] = paths
    dependencies[package["name"]] = [
        dependency["name"] for dependency in package["dependencies"]
    ]

affected = set()
for name, paths in sources.items():
    directories = {os.path.dirname(path) for path in paths}
    for changed in files:
        if changed in paths:
            affected.add(name)
            break
        # Un directorio cuenta solo si es el de los codigos del paquete:
        # el `build.rs` vive en la raiz del paquete y su directorio es
        # el del repositorio entero, y con el se creeria que TODO el
        # repositorio es de la API.
        conventional = {
            directory
            for directory in directories
            if os.path.basename(directory) in {"src", "tests", "benches", "examples"}
        }
        if any(changed.startswith(directory + "/") for directory in conventional):
            affected.add(name)
            break

# El cierre por dependencias: quien depende de lo que ha cambiado tambien
# ha cambiado.
growing = True
while growing:
    growing = False
    for name, deps in dependencies.items():
        if name in affected:
            continue
        if any(dep in affected for dep in deps):
            affected.add(name)
            growing = True

print(" ".join(sorted(affected)))
PYTHON
)"

# El contrato de la API es su propia puerta: si los fuentes del OpenAPI
# han cambiado, el gate del contrato se corre aunque no haya cambiado
# ningun crate.
if [ "${RUN_OPENAPI}" = "1" ]; then
    :
elif printf '%s\n' "${FILES}" | grep -q '^docs/openapi/'; then
    RUN_OPENAPI=1
fi

# ───────────────────────── las pruebas ─────────────────────────

if [ -z "${PACKAGES}" ]; then
    echo "los cambios no tocan ningun paquete de codigo"
else
    echo "paquetes afectados: ${PACKAGES}"
fi

if [ "${RUN_OPENAPI}" = "1" ]; then
    echo "── el contrato de la API ──"
    bash scripts/check-openapi.sh
fi

if [ "${RUN_CLIPPY}" = "1" ]; then
    # El formato se pide por PAQUETE y no por el workspace entero: el
    # `cargo fmt --all` recorre todos los archivos del repositorio y
    # tarda lo que tarda, mientras que el de un crate son unos
    # segundos. La puerta del dia a dia no puede ser la de la entrega.
    for package in ${PACKAGES}; do
        echo "── formato de ${package} ──"
        bounded cargo fmt -p "${package}" -- --check
    done
    for package in ${PACKAGES}; do
        echo "── avisos de ${package} ──"
        bounded cargo clippy -p "${package}" --all-targets -- -D warnings
    done
fi

if [ "${RUN_TESTS}" = "1" ]; then
    for package in ${PACKAGES}; do
        echo "── pruebas de ${package} ──"
        bounded cargo test -p "${package}"
    done
fi

echo "todo lo afectado esta en verde"
