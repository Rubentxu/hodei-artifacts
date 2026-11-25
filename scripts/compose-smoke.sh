#!/usr/bin/env bash
# Executable smoke test for compose.yaml and compose.external.yaml (HSP-258).
#
# Verifies against a real container runtime (Podman + a compose provider):
#   1. fail closed: the graph profile (compose.yaml, RUN_MODE=dgraph) with an
#      unreachable Dgraph Alpha and object store never becomes ready, exits
#      non-zero, mounts no /app/data and is not rejected as unsupported;
#   2. static: the compose files and compose.env.example carry no credentials
#      and validate.
# HSP-314 F2: the standalone (RocksDB) scenarios were removed with the embedded
# profile; the happy path of the graph profile is covered by hsp237/hsp308.
#
# Requires the image built by `scripts/package-oci.sh` (localhost/hodei-artifacts-api:latest).
# Exit codes: 0 = all checks passed, 1 = a check failed, 77 = NOT_RUN (no runtime/image).
#
# Usage: scripts/compose-smoke.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

IMAGE="${HODEI_IMAGE:-localhost/hodei-artifacts-api:latest}"
PODMAN_STORAGE_DRIVER="${PODMAN_STORAGE_DRIVER:-vfs}"
PODMAN_ROOT="${PODMAN_ROOT:-/var/tmp/podman-root}"
PODMAN_RUNROOT="${PODMAN_RUNROOT:-/var/tmp/podman-runroot}"
PODMAN_CMD=(podman --storage-driver="$PODMAN_STORAGE_DRIVER" --root "$PODMAN_ROOT" --runroot "$PODMAN_RUNROOT")

fail() { echo "FAIL: $*" >&2; exit 1; }
not_run() { echo "NOT_RUN: $*" >&2; exit 77; }
pass() { echo "ok: $*"; }

command -v podman >/dev/null 2>&1 || not_run "podman is not installed"
COMPOSE_BIN="$(command -v docker-compose || command -v podman-compose || true)"
[ -n "$COMPOSE_BIN" ] || not_run "no compose provider (docker-compose / podman-compose) found"
"${PODMAN_CMD[@]}" image exists "$IMAGE" 2>/dev/null \
  || not_run "image $IMAGE not found in $PODMAN_ROOT (run scripts/package-oci.sh first)"

WORK="$(mktemp -d)"
OVERRIDE="$WORK/runtime-override.yaml"
printf 'services:\n  hodei-artifacts-api: {}\n' > "$OVERRIDE"
SOCK="$WORK/podman.sock"
PROJECT="hodei-smoke-$$"
SERVICE_PID=""

cleanup() {
  set +e
  for f in compose.yaml compose.external.yaml; do
    compose -f "$ROOT/$f" -f "$OVERRIDE" down -v --remove-orphans >/dev/null 2>&1
  done
  [ -n "$SERVICE_PID" ] && kill "$SERVICE_PID" >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

# Podman API socket on the same storage the image was built into.
"${PODMAN_CMD[@]}" system service --time 0 "unix://$SOCK" >"$WORK/service.log" 2>&1 &
SERVICE_PID=$!
for _ in $(seq 1 50); do [ -S "$SOCK" ] && break; sleep 0.2; done
[ -S "$SOCK" ] || not_run "podman API socket did not come up (see $WORK/service.log)"

# Runtime-only override (not part of the shipped compose files). When the caller
# runs with a non-zero oom_score_adj (CI runners, sandboxes), crun cannot lower it
# for `exec`/healthchecks, so the service must inherit the same value.
OVERRIDE="$WORK/runtime-override.yaml"
OOM_ADJ="$(cat /proc/self/oom_score_adj 2>/dev/null || echo 0)"
{
  echo "services:"
  echo "  hodei-artifacts-api:"
  echo "    oom_score_adj: $OOM_ADJ"
} > "$OVERRIDE"

compose() { DOCKER_HOST="unix://$SOCK" "$COMPOSE_BIN" -p "$PROJECT" "$@"; }

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}

export HODEI_IMAGE="$IMAGE"
export HODEI_BIND_ADDR=127.0.0.1
export HODEI_PORT="$(free_port)"
# Throwaway, non-secret placeholders: only used to satisfy required variables.
export GARAGE_RPC_SECRET=smoke-placeholder-rpc
export GARAGE_ADMIN_TOKEN=smoke-placeholder-admin
export AWS_ACCESS_KEY_ID=smoke-placeholder-key
export AWS_SECRET_ACCESS_KEY=smoke-placeholder-secret
# Placeholders that satisfy the required variables of the graph profile; the
# fail-closed case below points them at a closed port.
export HODEI_GRAPH__ENDPOINT=127.0.0.1:1
export HODEI_GRAPH__CREDENTIALS__PASSWORD=smoke-placeholder-secret
export HODEI_CONTENT__S3__BUCKET=smoke-bucket
export HODEI_CONTENT__S3__ENDPOINT=127.0.0.1:1
export HODEI_DGRAPH_HMAC_SECRET_FILE="$WORK/hmac-secret"
: > "$HODEI_DGRAPH_HMAC_SECRET_FILE"
# Ephemeral signing key of the Terraform mirror: generated here, readable by the
# container user, and deleted with $WORK. Never a fixed value.
export HODEI_SIGNING_KEY_FILE="$WORK/signing-key"
if command -v openssl >/dev/null 2>&1; then
  openssl rand -hex 32 > "$HODEI_SIGNING_KEY_FILE"
else
  head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$HODEI_SIGNING_KEY_FILE"
fi
chmod 0444 "$HODEI_SIGNING_KEY_FILE"

http_status() {
  # Prints the HTTP status line code for GET $1 on the published port, or 000.
  curl -s -o /dev/null -w '%{http_code}' --max-time 3 "http://127.0.0.1:${HODEI_PORT}$1" || true
}

wait_ready() {
  for _ in $(seq 1 60); do
    [ "$(http_status /health/ready)" = "200" ] && return 0
    sleep 1
  done
  compose -f "$ROOT/compose.yaml" -f "$OVERRIDE" logs --no-color 2>&1 | tail -30 >&2 || true
  return 1
}

cid_of() { compose -f "$ROOT/$1" -f "$OVERRIDE" ps -a -q "$2" | head -n1; }

# --- 4 (static, cheap): no credentials in compose files or the example env ----
# The scanner also flags literals inside `${NAME:-literal}` defaults and short literals.
python3 scripts/compose-scan-secrets.py compose.yaml compose.external.yaml compose.env.example \
  || fail "a credential-like literal was found in compose files or compose.env.example"
pass "no credential literals in compose.yaml, compose.external.yaml, compose.env.example"
if grep -nE '^[^#]*(rocksdb|cas_path|CAS_PATH|/app/data)' compose.external.yaml; then
  fail "compose.external.yaml must not reference RocksDB or filesystem CAS paths"
fi
pass "compose.external.yaml has no RocksDB / CAS path"

compose -f compose.yaml -f "$OVERRIDE" config -q || fail "compose.yaml does not validate"
compose -f compose.external.yaml -f "$OVERRIDE" config -q || fail "compose.external.yaml does not validate"
pass "both compose files validate"

# --- 1. fail closed: the graph profile (compose.yaml) without its backends -----
# HSP-314 F2: the productive profile is the Dgraph one, and the API must never
# become ready, nor fall back to RocksDB or the filesystem, when the Alpha and the
# object store are unreachable. Only the API is started; the endpoints point at a
# closed loopback port.
cat > "$WORK/unreachable-override.yaml" <<'YAML'
services:
  hodei-artifacts-api:
    # Observe the first failure instead of the restart loop.
    restart: "no"
YAML
export HODEI_GRAPH__ENDPOINT=127.0.0.1:1
export HODEI_GRAPH__TLS__ENABLED=false
export HODEI_GRAPH__CREDENTIALS__PASSWORD=smoke-placeholder-secret
export HODEI_CONTENT__S3__BUCKET=unreachable-bucket
export HODEI_CONTENT__S3__ENDPOINT=127.0.0.1:1
export HODEI_CONTENT__S3__TLS=false
compose -f compose.yaml -f "$OVERRIDE" -f "$WORK/unreachable-override.yaml" up -d >/dev/null 2>&1 || true
CID="$(cid_of compose.yaml hodei-artifacts-api)"
[ -n "$CID" ] || fail "graph profile API container was not created"
STATE=""
for _ in $(seq 1 60); do
  STATE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.Status}}' "$CID")"
  [ "$STATE" = "exited" ] && break
  sleep 1
done
CODE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.ExitCode}}' "$CID")"
[ "$STATE" = "exited" ] && [ "$CODE" != "0" ] \
  || fail "the graph profile did not fail closed (state=$STATE exit=$CODE)"
[ "$(http_status /health/ready)" != "200" ] || fail "the graph profile answered ready without a backend"
MOUNTS="$("${PODMAN_CMD[@]}" inspect --format '{{range .Mounts}}{{.Destination}} {{end}}' "$CID")"
case "$MOUNTS" in *"/app/data"*) fail "the graph profile mounts /app/data: $MOUNTS" ;; esac
# The configuration must have been accepted: a rejected profile is a different
# failure from a backend that is merely down.
CLASS="$("${PODMAN_CMD[@]}" logs "$CID" 2>&1 | scripts/compose-classify-failure.sh)"
[ "$CLASS" != "unsupported-backend" ] \
  || fail "the graph profile was rejected as an unsupported backend: classified '$CLASS'"
compose -f compose.yaml -f "$OVERRIDE" down -v --remove-orphans >/dev/null
pass "fail closed: graph profile exits non-zero (exit=$CODE), classified $CLASS (not unsupported-backend), no /app/data mount, never ready"

echo "compose smoke: all checks passed"
echo "limitation: the happy path of compose.external.yaml (Dgraph + Garage serving traffic) is NOT_RUN here;"
echo "            it needs the Garage layout/bucket/key and the Alpha ACL bootstrap, see scripts/hsp308-oci-graph-e2e.sh."
