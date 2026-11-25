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

helm lint "$chart" >/dev/null && check "lint default" ok || check "lint default" no

std=$(helm template t "$chart")
rg=$(helm template t "$chart" -f "$chart/values-rocksdb-garage.yaml")
ext=$(helm template t "$chart" -f "$chart/values-external.yaml")
rgf=$(helm template t "$chart" -f "$chart/values-rocksdb-garage.yaml" --set replicaCount=5)

q() { yq -r "$2" <<<"$1" | { grep -vE '^(---)?$' || true; }; }
dep='select(.kind=="Deployment")'

for p in "standalone:$std" "rocksdb-garage:$rg"; do
  n=${p%%:*}; m=${p#*:}
  check "$n: Recreate" "$([ "$(q "$m" "$dep | .spec.strategy.type")" = Recreate ] && echo ok || echo no)"
  check "$n: 1 replica" "$([ "$(q "$m" "$dep | .spec.replicas")" = 1 ] && echo ok || echo no)"
  check "$n: fsGroup 10001" "$([ "$(q "$m" "$dep | .spec.template.spec.securityContext.fsGroup")" = 10001 ] && echo ok || echo no)"
  check "$n: RWO PVC" "$([ "$(q "$m" 'select(.kind=="PersistentVolumeClaim") | .spec.accessModes[0]')" = ReadWriteOnce ] && echo ok || echo no)"
done
check "rocksdb-garage ignores replicaCount=5" "$([ "$(q "$rgf" "$dep | .spec.replicas")" = 1 ] && echo ok || echo no)"

check "external: replicas >= 2" "$([ "$(q "$ext" "$dep | .spec.replicas")" -ge 2 ] && echo ok || echo no)"
check "external: no PVC" "$([ -z "$(q "$ext" 'select(.kind=="PersistentVolumeClaim") | .kind')" ] && echo ok || echo no)"
check "external: no persistentVolumeClaim volume" "$([ -z "$(q "$ext" "$dep | .spec.template.spec.volumes[] | select(.persistentVolumeClaim) | .name")" ] && echo ok || echo no)"
check "external: RollingUpdate" "$([ "$(q "$ext" "$dep | .spec.strategy.type")" = RollingUpdate ] && echo ok || echo no)"
for p in "standalone:$std" "rocksdb-garage:$rg" "external:$ext"; do
  n=${p%%:*}; m=${p#*:}
  check "$n: liveness /health/live" "$([ "$(q "$m" "$dep | .spec.template.spec.containers[0].livenessProbe.httpGet.path")" = /health/live ] && echo ok || echo no)"
  check "$n: readiness /health/ready" "$([ "$(q "$m" "$dep | .spec.template.spec.containers[0].readinessProbe.httpGet.path")" = /health/ready ] && echo ok || echo no)"
  check "$n: runAsUser 10001, non-root" "$([ "$(q "$m" "$dep | .spec.template.spec.securityContext.runAsUser")" = 10001 ] && echo ok || echo no)"
  check "$n: no Secret rendered" "$([ -z "$(q "$m" 'select(.kind=="Secret") | .kind')" ] && echo ok || echo no)"
done

sts='select(.kind=="StatefulSet" and .metadata.name=="t-hodei-artifacts-garage")'
check "garage: two volumeClaimTemplates (meta, data)" "$([ "$(q "$rg" "$sts | [.spec.volumeClaimTemplates[].metadata.name] | join(\",\")")" = meta,data ] && echo ok || echo no)"
check "garage: both RWO" "$([ "$(q "$rg" "$sts | [.spec.volumeClaimTemplates[].spec.accessModes[0]] | unique | join(\",\")")" = ReadWriteOnce ] && echo ok || echo no)"
job='select(.kind=="ConfigMap" and .metadata.name=="t-hodei-artifacts-garage-bootstrap") | .data."bootstrap.sh"'
for s in UpdateClusterLayout ApplyClusterLayout CreateBucket ImportKey AllowBucketKey; do
  check "bootstrap script: $s" "$(grep -q "$s" <<<"$(q "$rg" "$job")" && echo ok || echo no)"
done
check "bootstrap Job reads key from Secret" "$([ "$(q "$rg" 'select(.kind=="Job") | .spec.template.spec.containers[0].env[] | select(.name=="SECRET_ACCESS_KEY") | .valueFrom.secretKeyRef.name')" = hodei-s3 ] && echo ok || echo no)"
check "external: dgraph Zero/Alpha StatefulSets with PVC templates" "$([ "$(q "$ext" 'select(.kind=="StatefulSet" and (.metadata.name|test("dgraph"))) | .spec.volumeClaimTemplates[0].metadata.name' | sort | tr "\n" ,)" = alpha-data,zero-data, ] && echo ok || echo no)"

check "invalid profile fails" "$([ "$(yes helm template t "$chart" --set profile=bogus)" = no ] && echo ok || echo no)"
check "garage without Secret name fails" "$([ "$(yes helm template t "$chart" --set garage.enabled=true --set s3.existingSecret=x)" = no ] && echo ok || echo no)"

if command -v kubeconform >/dev/null 2>&1; then
  for m in "$std" "$rg" "$ext"; do
    echo "$m" | kubeconform -strict -summary -ignore-missing-schemas - >/dev/null && check "kubeconform" ok || check "kubeconform" no
  done
else
  echo "NOT_RUN - kubeconform is not installed"
fi

# --- external profile with the in-cluster Garage enabled (HSP-269) ---
extg=$(helm template t "$chart" -f "$chart/values-external.yaml" --set garage.enabled=true --set garage.existingSecret=hodei-s3)
gsts='select(.kind=="StatefulSet" and .metadata.name=="t-hodei-artifacts-garage")'
check "external+garage: Garage StatefulSet and bootstrap Job rendered" "$([ "$(q "$extg" 'select(.kind=="Job") | .metadata.name')" = t-hodei-artifacts-garage-bootstrap ] && [ "$(q "$extg" "$gsts | .metadata.name")" = t-hodei-artifacts-garage ] && echo ok || echo no)"
check "external+garage: Garage has two volumeClaimTemplates (meta, data)" "$([ "$(q "$extg" "$gsts | [.spec.volumeClaimTemplates[].metadata.name] | join(\",\")")" = meta,data ] && echo ok || echo no)"
check "external+garage: both Garage PVCs RWO" "$([ "$(q "$extg" "$gsts | [.spec.volumeClaimTemplates[].spec.accessModes[0]] | unique | join(\",\")")" = ReadWriteOnce ] && echo ok || echo no)"
check "external+garage: API has no PVC and no PVC volume" "$([ -z "$(q "$extg" 'select(.kind=="PersistentVolumeClaim") | .kind')" ] && [ -z "$(q "$extg" "$dep | .spec.template.spec.volumes[]? | select(.persistentVolumeClaim) | .name")" ] && echo ok || echo no)"
check "external+garage: no Secret rendered" "$([ -z "$(q "$extg" 'select(.kind=="Secret") | .kind')" ] && echo ok || echo no)"

# --- signing keys of the Terraform mirror (HSP-313) ---
sig=$(helm template t "$chart" --set signing.existingSecret=hodei-signing)
envof() { q "$1" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"$2\")"; }
check "signing: no key without a Secret (the mirror fails closed)" "$([ -z "$(envof "$std" HODEI_SIGNING__KEYS__0__ID)" ] && echo ok || echo no)"
check "signing: key id and secret reference rendered from the Secret" "$([ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING_KEY_SECRET_0\") | .valueFrom.secretKeyRef.name")" = hodei-signing ] && [ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING__KEYS__0__SECRET_ENV\") | .value")" = HODEI_SIGNING_KEY_SECRET_0 ] && [ "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING__KEYS__0__ID\") | .value")" = primary ] && echo ok || echo no)"
check "signing: the secret value is never inline" "$([ -z "$(q "$sig" "$dep | .spec.template.spec.containers[0].env[] | select(.name==\"HODEI_SIGNING_KEY_SECRET_0\") | .value // \"\"")" ] && echo ok || echo no)"
check "signing: still no Secret rendered" "$([ -z "$(q "$sig" 'select(.kind=="Secret") | .kind')" ] && echo ok || echo no)"
sig2=$(helm template t "$chart" --set signing.existingSecret=hodei-signing --set 'signing.keys[0].id=b' --set 'signing.keys[0].secretKey=kb' --set 'signing.keys[1].id=a' --set 'signing.keys[1].secretKey=ka')
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
