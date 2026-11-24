#!/usr/bin/env bash
# Manifest contract tests for charts/hodei-artifacts (HSP-264).
# Needs helm and yq; runs kubeconform when it is installed. Never touches a cluster.
set -euo pipefail

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

exit "$fail"
