#!/usr/bin/env bash
# Manifest contract tests for charts/hodei-artifacts (HSP-264).
# Needs helm, yq, python3 and curl; runs kubeconform when it is installed.
# Never touches a cluster. Exit codes: 0 pass, 1 failure, 77 NOT_RUN (a tool is missing).
set -euo pipefail
[ -z "${HELM_TEST_TRACE:-}" ] || set -x

for tool in helm yq python3 curl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "NOT_RUN: $tool is not installed, the chart contract tests did not run"
    exit 77
  fi
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chart="$root/charts/hodei-artifacts"
fail=0
check() { # description, condition result
  if [ "$2" = ok ]; then echo "ok   - $1"; else echo "FAIL - $1" >&2; fail=1; fi
}
yes() { if "$@" >/dev/null 2>&1; then echo ok; else echo no; fi; }

# HSP-314 F2: the graph profile is the only one. The credentials Secret is
# required (the ACL password is never rendered by the chart) and the Alpha is
# either the in-cluster one or a declared endpoint.
creds=(--set graph.credentials.existingSecret=hodei-dgraph-credentials)
helm lint "$chart" "${creds[@]}" --set graph.endpoint=alpha.example.invalid:9080 >/dev/null && check "lint default" ok || check "lint default" no

base=$(helm template t "$chart" "${creds[@]}" --set graph.endpoint=alpha.example.invalid:9080)
ext=$(helm template t "$chart" -f "$chart/values-graph.yaml")
rg=$(helm template t "$chart" "${creds[@]}" --set graph.endpoint=alpha.example.invalid:9080 --set garage.enabled=true --set garage.existingSecret=hodei-s3 --set s3.existingSecret=hodei-s3)
std=$base

q() { yq -r "$2" <<<"$1" | { grep -vE '^(---)?$' || true; }; }
dep='select(.kind=="Deployment")'
envof() { q "$1" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"$2\")"; }
envval() { q "$1" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"$2\") | .value"; }

for p in "graph:$base" "graph+dgraph:$ext" "graph+garage:$rg"; do
  n=${p%%:*}; m=${p#*:}
  check "$n: RUN_MODE=dgraph" "$([ "$(envval "$m" RUN_MODE)" = dgraph ] && echo ok || echo no)"
  check "$n: no RocksDB or CAS path in the environment" "$([ -z "$(envof "$m" HODEI_ROCKSDB__PATH)" ] && [ -z "$(envof "$m" HODEI_CONTENT__CAS_PATH)" ] && echo ok || echo no)"
  check "$n: content backend is s3" "$([ "$(envval "$m" HODEI_CONTENT__BACKEND)" = s3 ] && echo ok || echo no)"
  check "$n: ACL password comes from a Secret, never inline" "$([ -z "$(q "$m" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_GRAPH__CREDENTIALS__PASSWORD\") | .value // \"\"")" ] && [ "$(q "$m" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_GRAPH__CREDENTIALS__PASSWORD\") | .valueFrom.secretKeyRef.name")" = hodei-dgraph-credentials ] && echo ok || echo no)"
  check "$n: RollingUpdate" "$([ "$(q "$m" "$dep | .spec.strategy.type")" = RollingUpdate ] && echo ok || echo no)"
  check "$n: no PVC and no persistentVolumeClaim volume for the API" "$([ -z "$(q "$m" 'select(.kind=="PersistentVolumeClaim") | .kind')" ] && [ -z "$(q "$m" "$dep | .spec.template.spec.volumes[]? | select(.persistentVolumeClaim) | .name")" ] && echo ok || echo no)"
  check "$n: liveness /health/live" "$([ "$(q "$m" "$dep | .spec.template.spec.containers[0].livenessProbe.httpGet.path")" = /health/live ] && echo ok || echo no)"
  check "$n: readiness /health/ready" "$([ "$(q "$m" "$dep | .spec.template.spec.containers[0].readinessProbe.httpGet.path")" = /health/ready ] && echo ok || echo no)"
  check "$n: runAsUser 10001, non-root" "$([ "$(q "$m" "$dep | .spec.template.spec.securityContext.runAsUser")" = 10001 ] && echo ok || echo no)"
  check "$n: no Secret rendered" "$([ -z "$(q "$m" 'select(.kind=="Secret") | .kind')" ] && echo ok || echo no)"
done
check "graph: declared endpoint reaches the API" "$([ "$(envval "$base" HODEI_GRAPH__ENDPOINT)" = alpha.example.invalid:9080 ] && echo ok || echo no)"
check "graph+dgraph: the API points at the in-cluster Alpha" "$([ "$(envval "$ext" HODEI_GRAPH__ENDPOINT)" = t-hodei-artifacts-dgraph-alpha:9080 ] && echo ok || echo no)"
check "graph: replicas >= 2 by default" "$([ "$(q "$base" "$dep | .spec.replicas")" -ge 2 ] && echo ok || echo no)"

sts='select(.kind=="StatefulSet" and .metadata.name=="t-hodei-artifacts-garage")'
check "garage: two volumeClaimTemplates (meta, data)" "$([ "$(q "$rg" "$sts | [.spec.volumeClaimTemplates[].metadata.name] | join(\",\")")" = meta,data ] && echo ok || echo no)"
check "garage: both RWO" "$([ "$(q "$rg" "$sts | [.spec.volumeClaimTemplates[].spec.accessModes[0]] | unique | join(\",\")")" = ReadWriteOnce ] && echo ok || echo no)"
job='select(.kind=="ConfigMap" and .metadata.name=="t-hodei-artifacts-garage-bootstrap") | .data."bootstrap.sh"'
for s in UpdateClusterLayout ApplyClusterLayout CreateBucket ImportKey AllowBucketKey; do
  check "bootstrap script: $s" "$(grep -q "$s" <<<"$(q "$rg" "$job")" && echo ok || echo no)"
done
check "bootstrap Job reads key from Secret" "$([ "$(q "$rg" 'select(.kind=="Job") | .spec.template.spec.containers[0].env[] | select(.name=="SECRET_ACCESS_KEY") | .valueFrom.secretKeyRef.name')" = hodei-s3 ] && echo ok || echo no)"
check "graph+dgraph: Zero/Alpha StatefulSets with PVC templates" "$([ "$(q "$ext" 'select(.kind=="StatefulSet" and (.metadata.name|test("dgraph"))) | .spec.volumeClaimTemplates[0].metadata.name' | sort | tr "\n" ,)" = alpha-data,zero-data, ] && echo ok || echo no)"

check "removed profile standalone fails" "$([ "$(yes helm template t "$chart" "${creds[@]}" --set profile=standalone)" = no ] && echo ok || echo no)"
check "removed profile rocksdb-garage fails" "$([ "$(yes helm template t "$chart" "${creds[@]}" --set profile=rocksdb-garage)" = no ] && echo ok || echo no)"
check "invalid profile fails" "$([ "$(yes helm template t "$chart" "${creds[@]}" --set profile=bogus)" = no ] && echo ok || echo no)"
check "no credentials Secret fails (the ACL password is never rendered)" "$([ "$(yes helm template t "$chart" --set graph.endpoint=a:1)" = no ] && echo ok || echo no)"
check "no Alpha endpoint fails without the in-cluster Dgraph" "$([ "$(yes helm template t "$chart" "${creds[@]}")" = no ] && echo ok || echo no)"
check "garage without Secret name fails" "$([ "$(yes helm template t "$chart" "${creds[@]}" --set graph.endpoint=a:1 --set garage.enabled=true --set s3.existingSecret=x)" = no ] && echo ok || echo no)"

if command -v kubeconform >/dev/null 2>&1; then
  for m in "$base" "$rg" "$ext"; do
    echo "$m" | kubeconform -strict -summary -ignore-missing-schemas - >/dev/null && check "kubeconform" ok || check "kubeconform" no
  done
else
  echo "NOT_RUN - kubeconform is not installed"
fi

# --- signing keys of the Terraform mirror (HSP-313) ---
sig=$(helm template t "$chart" "${creds[@]}" --set graph.endpoint=a:1 --set signing.existingSecret=hodei-signing)
check "signing: no key without a Secret (the mirror fails closed)" "$([ -z "$(envof "$std" HODEI_SIGNING__KEYS__0__ID)" ] && echo ok || echo no)"
check "signing: key id and secret reference rendered from the Secret" "$([ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING_KEY_SECRET_0\") | .valueFrom.secretKeyRef.name")" = hodei-signing ] && [ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING__KEYS__0__SECRET_ENV\") | .value")" = HODEI_SIGNING_KEY_SECRET_0 ] && [ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING__KEYS__0__ID\") | .value")" = primary ] && echo ok || echo no)"
check "signing: the secret value is never inline" "$([ -z "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING_KEY_SECRET_0\") | .value // \"\"")" ] && echo ok || echo no)"
check "signing: still no Secret rendered" "$([ -z "$(q "$sig" 'select(.kind=="Secret") | .kind')" ] && echo ok || echo no)"
sig2=$(helm template t "$chart" "${creds[@]}" --set graph.endpoint=a:1 --set signing.existingSecret=hodei-signing --set 'signing.keys[0].id=b' --set 'signing.keys[0].secretKey=kb' --set 'signing.keys[1].id=a' --set 'signing.keys[1].secretKey=ka')
check "signing: a ring of two keys keeps the order (index 0 signs)" "$([ "$(q "$sig2" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING__KEYS__1__ID\") | .value")" = a ] && echo ok || echo no)"

# --- helm package is reproducible only with SOURCE_DATE_EPOCH (HSP-269) ---
# Without it the archive embeds the mtime of every file, so two checkouts differ.
work=$(mktemp -d)
double_pid=""
cleanup() {
  [ -z "$double_pid" ] || kill "$double_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT
sha_of_copy() { # mtime-date -> sha256 of the package built from a fresh copy with that mtime
  local d="$work/pkg-$1"
  mkdir -p "$d/out"
  cp -r "$chart" "$d/hodei-artifacts"
  find "$d/hodei-artifacts" -exec touch -d "$1" {} +
  SOURCE_DATE_EPOCH=1700000000 helm package "$d/hodei-artifacts" -d "$d/out" >/dev/null
  sha256sum "$d"/out/*.tgz | cut -d' ' -f1
}
sha_a=$(sha_of_copy 2001-01-01)
sha_b=$(sha_of_copy 2020-05-05)
check "helm package: identical sha256 across copies with different mtimes (SOURCE_DATE_EPOCH)" "$([ -n "$sha_a" ] && [ "$sha_a" = "$sha_b" ] && echo ok || echo no)"

# --- bootstrap.sh against a local double of the Garage admin API v2 (HSP-269) ---
script="$work/bootstrap.sh"
q "$rg" "$job" > "$script"
double="$root/scripts/garage-admin-double.py"
run_bootstrap() { # label, double args... -> sets rc and leaves the request log in $work/log
  local label=$1; shift
  rm -f "$work/port" "$work/log"
  python3 "$double" "$work/port" "$work/log" tok "$@" &
  double_pid=$!
  for _ in $(seq 100); do [ -s "$work/port" ] && break; sleep 0.1; done
  [ -s "$work/port" ] || { echo "FAIL - $label: double did not start" >&2; fail=1; return; }
  sed -e "s#^\( *API_BASE=\).*#\1\"http://127.0.0.1:$(cat "$work/port")\"#" \
      -e "s#/tmp/#$work/#g" -e 's#sleep 2#sleep 0#' "$script" > "$work/run.sh"
}
stop_double() { kill "$double_pid" 2>/dev/null || true; wait "$double_pid" 2>/dev/null || true; double_pid=""; }
boot() { # runs the extracted script once with the Secret values in the environment
  GARAGE_ADMIN_TOKEN=tok BUCKET=hodei KEY_NAME=hodei LAYOUT_ZONE=dc1 LAYOUT_CAPACITY_BYTES=1000 \
    ACCESS_KEY_ID=GK0123456789abcdef01234567 SECRET_ACCESS_KEY=s3cret sh "$work/run.sh" >"$work/out" 2>&1 && echo 0 || echo $?
}
count() { grep -c "$1" "$work/log" || true; }

run_bootstrap "bootstrap"
rc1=$(boot)
mut1=$(grep -cE 'UpdateClusterLayout|ApplyClusterLayout|CreateBucket|ImportKey' "$work/log" || true)
cp "$work/log" "$work/log1"; : > "$work/log"
rc2=$(boot)
check "bootstrap: first pass exits 0 and creates layout, bucket and key" "$([ "$rc1" = 0 ] && [ "$mut1" = 4 ] && echo ok || echo no)"
check "bootstrap: 404 on bucket creates it, 404 on key imports it" "$([ "$(grep -c -E 'GET /v2/GetBucketInfo' "$work/log1")" = 1 ] && grep -q 'POST /v2/CreateBucket' "$work/log1" && grep -q 'GET /v2/GetKeyInfo' "$work/log1" && grep -q 'POST /v2/ImportKey' "$work/log1" && echo ok || echo no)"
check "bootstrap: second pass exits 0 without UpdateClusterLayout, CreateBucket or ImportKey" "$([ "$rc2" = 0 ] && [ "$(count -E 'UpdateClusterLayout|ApplyClusterLayout|CreateBucket|ImportKey')" = 0 ] && grep -q 'layout already applied' "$work/out" && echo ok || echo no)"
stop_double

# The fresh double answers 404 to GetBucketInfo and GetKeyInfo: the first pass must react to both.
for ep in GetBucketInfo GetKeyInfo UpdateClusterLayout CreateBucket ImportKey; do
  run_bootstrap "5xx on $ep" --fail "$ep=503"
  rc=$(boot)
  check "bootstrap: 5xx on $ep exits non-zero" "$([ "$rc" != 0 ] && grep -q 'failed' "$work/out" && echo ok || echo no)"
  stop_double
done

exit "$fail"
