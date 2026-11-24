#!/usr/bin/env bash
# HSP-177: verify TLS enforcement on the single Zero+Alpha lab started with
#   podman compose -f compose.yaml -f compose-tls.yaml up -d
# Prints one PASS/FAIL line per check and exits non-zero on any FAIL.
# Assumes the stack is up, ./tls-single exists and the guardian password is
# the default ("password"); override with GROOT_PASSWORD.
set -uo pipefail
cd "$(dirname "$0")"

CA=tls-single/ca.crt
WORK="$(cd ../.. && pwd)/target/uat/hsp177/wrongca"
PASSWORD=${GROOT_PASSWORD:-password}
IMG=docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb
NET=dgraph-airgap
fail=0
check() { # name, expected(0=must succeed,1=must fail), command...
  local name=$1 want=$2; shift 2
  local out rc
  out=$("$@" 2>&1); rc=$?
  if { [ "$want" = 0 ] && [ $rc -eq 0 ]; } || { [ "$want" = 1 ] && [ $rc -ne 0 ]; }; then
    echo "PASS $name (rc=$rc)"
  else
    echo "FAIL $name (rc=$rc): $(echo "$out" | head -c 200)"; fail=1
  fi
}

mkdir -p "$WORK"
[ -f "$WORK/wrong-ca.crt" ] || openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$WORK/wrong.key" -out "$WORK/wrong-ca.crt" -subj "/CN=wrong-lab-ca" -days 2 2>/dev/null

# --- client <-> Alpha HTTP (8080)
check "http: correct CA, /state"               0 curl -sSf -m10 --cacert "$CA" https://localhost:8080/state
check "http: wrong CA rejected"                1 curl -sSf -m10 --cacert "$WORK/wrong-ca.crt" https://localhost:8080/state
check "http: no CA (system store) rejected"    1 curl -sSf -m10 https://localhost:8080/state
check "http: plain-text /state rejected"       1 curl -sSf -m10 http://localhost:8080/state
check "http: plain-text /login rejected"       1 curl -sSf -m10 -d "{\"userid\":\"groot\",\"password\":\"$PASSWORD\"}" http://localhost:8080/login
check "http: client cert from lab CA accepted" 0 curl -sSf -m10 --cacert "$CA" \
  --cert tls-single/client.hodei-client.crt --key tls-single/client.hodei-client.key https://localhost:8080/state
# Guardian login is loopback-only in Dgraph and the published port arrives
# from the bridge gateway, so the authenticated flow runs inside the Alpha's
# network namespace (source 127.0.0.1) over TLS with the lab CA.
check "http: authenticated login+query over TLS (alpha netns)" 0 \
  podman run --rm --network container:dgraph-alpha -v "$(pwd)/tls-single:/t:ro,z" --entrypoint bash "$IMG" -c '
  tok=$(curl -sSf -m10 --cacert /t/ca.crt -d "{\"userid\":\"groot\",\"password\":\"$0\"}" https://localhost:8080/login \
        | sed -n "s/.*\"accessJWT\":\"\([^\"]*\)\".*/\1/p") && [ -n "$tok" ] &&
  curl -sSf -m10 --cacert /t/ca.crt -H "X-Dgraph-AccessToken: $tok" -H "Content-Type: application/dql" \
       -d "{q(func:has(dgraph.xid)){dgraph.xid}}" https://localhost:8080/query | grep -q groot' "$PASSWORD"
check "http: same login over plain-text (alpha netns) rejected" 1 \
  podman run --rm --network container:dgraph-alpha --entrypoint bash "$IMG" -c \
  'curl -sSf -m10 -d "{\"userid\":\"groot\",\"password\":\"$0\"}" http://localhost:8080/login' "$PASSWORD"

# --- client <-> Alpha gRPC (9080): handshake only (openssl), no client lib needed
sclient() { echo | openssl s_client -connect localhost:9080 -servername localhost "$@" 2>&1; }
check "grpc: correct CA verifies (code 0)"     0 bash -c 'echo | openssl s_client -connect localhost:9080 -servername localhost -CAfile "$1" -verify_return_error 2>&1 | grep -q "Verify return code: 0"' _ "$CA"
check "grpc: wrong CA fails verification"      1 bash -c 'echo | openssl s_client -connect localhost:9080 -servername localhost -CAfile "$1" -verify_return_error 2>&1 | grep -q "Verify return code: 0"' _ "$WORK/wrong-ca.crt"
check "grpc: plain-text HTTP/2 rejected"       1 curl -sSf -m5 --http2-prior-knowledge http://localhost:9080/

# --- node <-> node (internal ports), probed from inside the closed network
probe() { podman run --rm --network "$NET" -v "$(pwd)/tls-single:/t:ro,z" --entrypoint bash "$IMG" -c "$1"; }
check "zero:5080 internal: lab CA verifies"    0 probe 'echo | openssl s_client -connect dgraph-zero:5080 -servername dgraph-zero -CAfile /t/ca.crt -verify_return_error 2>&1 | grep -q "Verify return code: 0"'
check "alpha:7080 internal: lab CA verifies"   0 probe 'echo | openssl s_client -connect dgraph-alpha:7080 -servername dgraph-alpha -CAfile /t/ca.crt -verify_return_error 2>&1 | grep -q "Verify return code: 0"'
check "zero:5080 internal: plain-text rejected" 1 probe 'curl -sSf -m5 --http2-prior-knowledge http://dgraph-zero:5080/'
check "alpha:7080 internal: plain-text rejected" 1 probe 'curl -sSf -m5 --http2-prior-knowledge http://dgraph-alpha:7080/'
check "cluster healthy (alpha joined zero over TLS)" 0 bash -c 'curl -sSf -m10 --cacert "$1" https://localhost:8080/health | grep -q "\"status\":\"healthy\""' _ "$CA"

exit $fail
