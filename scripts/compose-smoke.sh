#!/usr/bin/env bash
# Executable smoke test for compose.yaml and compose.external.yaml (HSP-258).
#
# Verifies against a real container runtime (Podman + a compose provider):
#   1. standalone: starts as non-root with a read-only rootfs, named volume,
#      and a passing /health/ready healthcheck;
#   2. standalone: data persists in the named volume across down/up;
#   3. fail closed: an unreachable S3 backend and the external (Dgraph + S3)
#      profile never become ready, exit non-zero and never touch RocksDB/CAS;
#   4. static: the compose files and compose.env.example carry no credentials.
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
  compose -f "$ROOT/compose.yaml" -f "$OVERRIDE" -f "$WORK/s3-override.yaml" down -v --remove-orphans >/dev/null 2>&1
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

# --- 1. standalone boots hardened and healthy ---------------------------------
compose -f compose.yaml -f "$OVERRIDE" up -d >/dev/null
wait_ready || fail "standalone did not become ready on /health/ready"
CID="$(cid_of compose.yaml hodei-artifacts-api)"
[ -n "$CID" ] || fail "standalone container id not found"
USER_CFG="$("${PODMAN_CMD[@]}" inspect --format '{{.Config.User}}' "$CID")"
[ "$USER_CFG" = "10001:10001" ] || fail "expected user 10001:10001, got '$USER_CFG'"
[ "$("${PODMAN_CMD[@]}" inspect --format '{{.HostConfig.ReadonlyRootfs}}' "$CID")" = "true" ] \
  || fail "rootfs is not read-only"
UID_RUNTIME="$("${PODMAN_CMD[@]}" top "$CID" user | tail -n +2 | head -n1 | tr -d '[:space:]')"
case "$UID_RUNTIME" in hodei|10001) ;; *) fail "process runs as '$UID_RUNTIME', expected hodei/10001" ;; esac
for _ in $(seq 1 60); do
  H="$("${PODMAN_CMD[@]}" inspect --format '{{.State.Health.Status}}' "$CID")"
  [ "$H" = "healthy" ] && break
  sleep 1
done
[ "$H" = "healthy" ] || fail "container healthcheck status is '$H', expected healthy"
pass "standalone: non-root uid 10001, read-only rootfs, /health/ready 200, healthcheck healthy"

# --- 2. persistence across down/up -------------------------------------------
# Inspect the named volume through a throwaway container (the volume is shared).
VOLUME="${PROJECT}_hodei-artifacts-data"
volume_run() {
  "${PODMAN_CMD[@]}" run --rm --user 10001:10001 -v "$VOLUME:/app/data" \
    --entrypoint /bin/bash "$IMAGE" -c "$1"
}
compose -f compose.yaml -f "$OVERRIDE" down >/dev/null
volume_run 'echo persisted > /app/data/smoke-marker' || fail "could not write to the named volume"
compose -f compose.yaml -f "$OVERRIDE" up -d >/dev/null
wait_ready || fail "standalone did not become ready after restart"
CID="$(cid_of compose.yaml hodei-artifacts-api)"
compose -f compose.yaml -f "$OVERRIDE" down >/dev/null
[ "$(volume_run 'cat /app/data/smoke-marker')" = "persisted" ] \
  || fail "marker lost after down/up: named volume is not persistent"
volume_run 'test -d /app/data/hodei.rocksdb' \
  || fail "RocksDB directory missing from the persisted volume"
pass "standalone: data persisted in the named volume across down/up"
compose -f compose.yaml -f "$OVERRIDE" down -v >/dev/null

# --- 3a. fail closed: unreachable S3 backend on the standalone profile --------
cat > "$WORK/s3-override.yaml" <<'YAML'
services:
  hodei-artifacts-api:
    # Observe the first failure instead of the restart loop.
    restart: "no"
    environment:
      HODEI_CONTENT__BACKEND: s3
      HODEI_CONTENT__S3__BUCKET: unreachable-bucket
      HODEI_CONTENT__S3__ENDPOINT: 127.0.0.1:1
      HODEI_CONTENT__S3__REGION: eu-west-1
      HODEI_CONTENT__S3__TLS: "false"
      HODEI_CONTENT__CAS_PATH: /app/data/forbidden-s3.cas
      AWS_ACCESS_KEY_ID: smoke-placeholder-key
      AWS_SECRET_ACCESS_KEY: smoke-placeholder-secret
YAML
compose -f compose.yaml -f "$OVERRIDE" -f "$WORK/s3-override.yaml" up -d >/dev/null 2>&1 || true
CID="$(compose -f compose.yaml -f "$OVERRIDE" -f "$WORK/s3-override.yaml" ps -a -q hodei-artifacts-api | head -n1)"
[ -n "$CID" ] || fail "S3 fail-closed container was not created"
STATE=""
for _ in $(seq 1 30); do
  STATE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.Status}}' "$CID")"
  [ "$STATE" = "exited" ] && break
  sleep 1
done
CODE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.ExitCode}}' "$CID")"
[ "$STATE" = "exited" ] && [ "$CODE" != "0" ] \
  || fail "unreachable S3 did not fail closed (state=$STATE exit=$CODE)"
[ "$(http_status /health/ready)" != "200" ] || fail "service answered ready with unreachable S3"
# The failure must be a connectivity failure, not a rejected configuration.
CLASS="$("${PODMAN_CMD[@]}" logs "$CID" 2>&1 | scripts/compose-classify-failure.sh)"
[ "$CLASS" = "connectivity" ] \
  || fail "unreachable S3 failed for the wrong reason: classified '$CLASS', expected 'connectivity'"
compose -f compose.yaml -f "$OVERRIDE" -f "$WORK/s3-override.yaml" down -v >/dev/null
pass "fail closed: unreachable S3 backend exits non-zero (exit=$CODE), classified connectivity, never reports ready"

# --- 3b. fail closed: external profile (Dgraph + S3) --------------------------
# Only the API is started (--no-deps): Dgraph/Garage are unreachable from it, and
# today the API also rejects db_type=dgraph. Either way it must fail closed.
printf 'services:\n  hodei-artifacts-api:\n    restart: "no"\n' > "$WORK/norestart.yaml"
compose -f compose.external.yaml -f "$OVERRIDE" -f "$WORK/norestart.yaml" up -d --no-deps hodei-artifacts-api >/dev/null 2>&1 || true
CID="$(cid_of compose.external.yaml hodei-artifacts-api)"
[ -n "$CID" ] || fail "external profile API container was not created"
STATE=""
for _ in $(seq 1 30); do
  STATE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.Status}}' "$CID")"
  [ "$STATE" = "exited" ] && break
  sleep 1
done
CODE="$("${PODMAN_CMD[@]}" inspect --format '{{.State.ExitCode}}' "$CID")"
[ "$STATE" = "exited" ] && [ "$CODE" != "0" ] \
  || fail "external profile did not fail closed (state=$STATE exit=$CODE)"
[ "$(http_status /health/ready)" != "200" ] || fail "external profile answered ready without a backend"
MOUNTS="$("${PODMAN_CMD[@]}" inspect --format '{{range .Mounts}}{{.Destination}} {{end}}' "$CID")"
case "$MOUNTS" in *"/app/data"*) fail "external profile mounts /app/data: $MOUNTS" ;; esac
# Today the API rejects db_type=dgraph before touching any backend, so the failure
# must be classified unsupported-backend and NOT connectivity: a backend that is
# merely down (Dgraph/Garage unreachable) is a different failure. When the Dgraph
# backend lands this assertion must change on purpose, not drift silently.
CLASS="$("${PODMAN_CMD[@]}" logs "$CID" 2>&1 | scripts/compose-classify-failure.sh)"
[ "$CLASS" = "unsupported-backend" ] \
  || fail "external profile failed for the wrong reason: classified '$CLASS', expected 'unsupported-backend'"
compose -f compose.external.yaml -f "$OVERRIDE" down -v --remove-orphans >/dev/null
pass "fail closed: external profile exits non-zero (exit=$CODE), classified unsupported-backend (not connectivity), no /app/data mount, never ready"

echo "compose smoke: all checks passed"
echo "limitation: the external profile's happy path (Dgraph + S3 serving traffic) is NOT_RUN;"
echo "            the API rejects db_type=dgraph today (only 'rocksdb' is supported)."
