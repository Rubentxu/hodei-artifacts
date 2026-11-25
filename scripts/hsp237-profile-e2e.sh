#!/usr/bin/env bash
#
# HSP-237 / HSP-292: recorrido funcional del perfil de grafo (Dgraph real + S3).
#
# Levanta con podman el Dgraph de deploy/dgraph/compose.yaml y un S3
# (LocalStack, como la serie HSP-138), arranca el binario real en perfil de
# grafo (RUN_MODE=dgraph, seccion [graph]) y mide /health/ready y
# /health/live al parar y devolver Dgraph y el S3.
# La evidencia queda en target/uat/hsp237-profile-e2e/ (gitignored).
#
# Uso: scripts/hsp237-profile-e2e.sh
# Salida: 0 PASS; 1 FAIL; 77 NOT_RUN (sin podman, sin imagenes o guarda no
# activada); 124 TIMEOUT (la guarda de tiempo; es un FAIL).
# Un NOT_RUN nunca es un PASS.
# Variables: HSP237_E2E_TIMEOUT_SECS (por defecto 1500; el primer arranque
# compila el binario y las pruebas), CARGO_TARGET_DIR, CARGO_BUILD_JOBS.
set -u
cd "$(dirname "$0")/.." || exit 77
out=target/uat/hsp237-profile-e2e
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

rm -f "$out/NOT_RUN.txt" "$out/report.md"
HSP237_PROFILE_E2E=1 \
  nice -n 10 ionice -c3 \
  timeout "${HSP237_E2E_TIMEOUT_SECS:-1500}" \
  cargo test -j"${CARGO_BUILD_JOBS:-6}" -p hodei-artifacts-api --test hsp237_profile_external_e2e -- --nocapture \
  2>&1 | tee "$out/run.log"
status=${PIPESTATUS[0]}
if [ "$status" -eq 0 ] && grep -q "SKIPPED" "$out/run.log"; then
  echo "NOT_RUN: la guardia HSP237_PROFILE_E2E no se activo" | tee "$out/NOT_RUN.txt"
  exit 77
fi
if [ "$status" -eq 124 ]; then
  # El timeout mata cargo sin ejecutar los Drop de la prueba: limpia el laboratorio.
  podman ps -a --format '{{.Names}}' | grep '^hsp237-' | xargs -r podman rm -f -t 0 >/dev/null 2>&1
  echo "FAIL: la guarda de tiempo (${HSP237_E2E_TIMEOUT_SECS:-1500}s) corto la ejecucion" | tee -a "$out/run.log"
fi
[ "$status" -eq 0 ] && echo PASS || echo FAIL
exit "$status"
