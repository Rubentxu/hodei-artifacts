#!/usr/bin/env bash
# HSP-177: formal SBOM (SPDX + CycloneDX) and CVE scan of the pinned Dgraph
# image, using syft and trivy run as podman containers (pinned by digest).
#
# Needs network ONCE: to pull the scanner images and the trivy vulnerability
# DB (the scan result therefore depends on the DB date; it is recorded).
# Large artifacts go to target/uat/hsp177/ (gitignored); a small summary is
# written to deploy/dgraph/evidence/.
set -euo pipefail
cd "$(dirname "$0")"

DGRAPH=docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb
SYFT=ghcr.io/anchore/syft@sha256:e97e20b6e6d1699c60296b66ab337941724f9bd06ac279d893eda89601fc4b2a   # 1.57.2
TRIVY=docker.io/aquasec/trivy@sha256:8f6bb947dbc528902e11f2f5be52f7b519f6a39d5d148f41166f2040499768fb # 0.76.2
OUT="$(cd ../.. && pwd)/target/uat/hsp177"
mkdir -p "$OUT/cache" evidence
LIM="nice -n 10 ionice -c3"

podman pull -q "$DGRAPH" >/dev/null
$LIM podman save --format docker-archive -o "$OUT/dgraph.tar" "$DGRAPH"
$LIM podman run --rm -v "$OUT:/w:z" "$SYFT" docker-archive:/w/dgraph.tar \
  -o spdx-json=/w/sbom.spdx.json -o cyclonedx-json=/w/sbom.cdx.json
$LIM podman run --rm -v "$OUT:/w:z" -v "$OUT/cache:/root/.cache:z" "$TRIVY" \
  image --quiet --no-progress --input /w/dgraph.tar --scanners vuln \
  --format json -o /w/trivy.json

python3 -I - "$OUT" evidence/sbom-cve-summary.json <<'PY'
import hashlib, json, sys, collections
out, dst = sys.argv[1], sys.argv[2]
sha = lambda f: hashlib.sha256(open(f"{out}/{f}", "rb").read()).hexdigest()
spdx = json.load(open(f"{out}/sbom.spdx.json"))
cdx = json.load(open(f"{out}/sbom.cdx.json"))
tv = json.load(open(f"{out}/trivy.json"))
sev = collections.Counter(); high = []
for r in tv.get("Results", []):
    for v in r.get("Vulnerabilities") or []:
        sev[v["Severity"]] += 1
        if v["Severity"] in ("HIGH", "CRITICAL"):
            high.append({"id": v["VulnerabilityID"], "pkg": v["PkgName"],
                         "installed": v["InstalledVersion"],
                         "fixed": v.get("FixedVersion"), "target": r["Target"]})
json.dump({
  "image": "docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb",
  "syft": "1.57.2", "trivy": "0.76.2",
  "spdx_version": spdx["spdxVersion"], "spdx_packages": len(spdx["packages"]),
  "cyclonedx_components": len(cdx["components"]),
  "sha256": {f: sha(f) for f in ("sbom.spdx.json", "sbom.cdx.json", "trivy.json")},
  "os": tv.get("Metadata", {}).get("OS"),
  "cve_by_severity": dict(sev), "cve_high_critical": high,
}, open(dst, "w"), indent=2)
print(open(dst).read())
PY
