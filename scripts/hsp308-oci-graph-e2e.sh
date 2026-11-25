#!/usr/bin/env bash
#
# HSP-308: la imagen OCI (no el binario de cargo) en el perfil Dgraph/S3.
#
# Levanta con podman un Dgraph real y un S3 (el laboratorio de HSP-237),
# arranca la imagen localhost/hodei-artifacts-api:latest con RUN_MODE=dgraph,
# escribe un artefacto, reinicia el contenedor y lo lee de vuelta con el
# mismo digest. La imagen NO se reconstruye aqui (una build release en frio
# tarda mas de 20 minutos): se construye antes con scripts/package-oci.sh, o
# se pide con HSP308_BUILD_IMAGE=1.
# La evidencia queda en target/uat/hsp308-oci-graph-e2e/ (gitignored).
#
# Uso: scripts/hsp308-oci-graph-e2e.sh
# Salida: 0 PASS; 1 FAIL; 77 NOT_RUN (sin podman, sin imagenes o sin la
# imagen de la API); 124 TIMEOUT (la guarda de tiempo; es un FAIL).
# Un NOT_RUN nunca es un PASS.
# Variables: HSP308_E2E_TIMEOUT_SECS (por defecto 300 sin build),
# HSP308_BUILD_IMAGE=1 (construye la imagen con package-oci.sh; entonces el
# tiempo por defecto sube a 3600), PODMAN_STORAGE_DRIVER / PODMAN_ROOT /
# PODMAN_RUNROOT (el almacen de la imagen, como en hsp242_oci_image),
# CARGO_TARGET_DIR, CARGO_BUILD_JOBS.
set -u
cd "$(dirname "$0")/.." || exit 77
out=target/uat/hsp308-oci-graph-e2e
mkdir -p "$out"

if ! command -v podman >/dev/null 2>&1; then
  echo "NOT_RUN: podman no esta disponible (command -v podman)" | tee "$out/NOT_RUN.txt"
  exit 77
fi
for image in \
  "docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb" \
  "docker.io/localstack/localstack:4.14"; do
  if ! podman image exists "$image"; then
    echo "NOT_RUN: falta la imagen local (podman pull $image)" | tee "$out/NOT_RUN.txt"
    exit 77
  fi
done

store=(--storage-driver="${PODMAN_STORAGE_DRIVER:-vfs}" --root "${PODMAN_ROOT:-/var/tmp/podman-root}" --runroot "${PODMAN_RUNROOT:-/var/tmp/podman-runroot}")
default_timeout=300
if [ "${HSP308_BUILD_IMAGE:-0}" = "1" ]; then
  default_timeout=3600
elif ! podman "${store[@]}" image exists localhost/hodei-artifacts-api:latest; then
  echo "NOT_RUN: falta localhost/hodei-artifacts-api:latest en el almacen de podman (scripts/package-oci.sh, o HSP308_BUILD_IMAGE=1)" | tee "$out/NOT_RUN.txt"
  exit 77
fi

rm -f "$out/NOT_RUN.txt"
if [ "${HSP308_BUILD_IMAGE:-0}" = "1" ]; then
  nice -n 10 ionice -c3 timeout "${HSP308_E2E_TIMEOUT_SECS:-$default_timeout}" \
    scripts/package-oci.sh "$(mktemp -d)" 2>&1 | tee "$out/build.log"
  [ "${PIPESTATUS[0]}" -eq 0 ] || { echo "FAIL: package-oci.sh"; exit 1; }
fi
HSP308_OCI_GRAPH_E2E=1 \
  nice -n 10 ionice -c3 \
  timeout "${HSP308_E2E_TIMEOUT_SECS:-$default_timeout}" \
  cargo test -j"${CARGO_BUILD_JOBS:-6}" -p hodei-artifacts-api --test hsp242_oci_image \
  graph_profile_image -- --nocapture \
  2>&1 | tee "$out/run.log"
status=${PIPESTATUS[0]}
if [ "$status" -eq 0 ] && grep -q "SKIPPED" "$out/run.log"; then
  echo "NOT_RUN: la guardia HSP308_OCI_GRAPH_E2E no se activo" | tee "$out/NOT_RUN.txt"
  exit 77
fi
if [ "$status" -eq 124 ]; then
  # El timeout mata cargo sin ejecutar los Drop de la prueba: limpia el laboratorio.
  podman ps -a --format '{{.Names}}' | grep '^hodei-hsp308-' | xargs -r podman rm -f -t 0 >/dev/null 2>&1
  podman "${store[@]}" ps -a --format '{{.Names}}' | grep '^hodei-hsp308-' | xargs -r podman "${store[@]}" rm -f -t 0 >/dev/null 2>&1
  echo "FAIL: la guarda de tiempo cortó la ejecucion" | tee -a "$out/run.log"
fi
[ "$status" -eq 0 ] && echo PASS || echo FAIL
exit "$status"
