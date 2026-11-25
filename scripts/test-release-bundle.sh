#!/usr/bin/env bash
# Suite for scripts/release-bundle.sh and the bundle mode of
# scripts/verify-release.sh.
#
# The expensive stages (cargo build, podman image build, syft) are replaced by
# stubs that emit the same layout as scripts/package-release.sh and
# scripts/package-oci.sh and sign with the shared OpenPGP key handed over by
# the orchestrator. Everything the orchestrator itself owns (formula, bundle
# manifest, NOT_RUN accounting, SHA256SUMS regeneration, signatures and the
# final clean-directory verification) runs for real.
#
# Usage: scripts/test-release-bundle.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_SH="$ROOT/scripts/release-bundle.sh"
VERIFY="$ROOT/scripts/verify-release.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { PASS=$((PASS + 1)); echo "ok - $*"; }

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
ARCH="$(uname -m)"
BIN="hodei-artifacts-api"
TARBALL="$BIN-$VERSION-$ARCH-unknown-linux-gnu.tar.gz"
MANIFEST="$BIN-$VERSION-$ARCH.manifest.json"
OCI="$BIN-$VERSION-$ARCH-oci"
COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"

# --- Stubs ------------------------------------------------------------------

cat > "$WORK/stub-tarball.sh" <<STUB
#!/usr/bin/env bash
set -euo pipefail
out="\$1"; mkdir -p "\$out/pkg/$BIN-$VERSION-$ARCH-unknown-linux-gnu/bin"
printf '#!/bin/sh\necho stub\n' > "\$out/pkg/$BIN-$VERSION-$ARCH-unknown-linux-gnu/bin/$BIN"
tar -C "\$out/pkg" -czf "\$out/$TARBALL" "$BIN-$VERSION-$ARCH-unknown-linux-gnu"
rm -rf "\$out/pkg"
( cd "\$out" && sha256sum "$TARBALL" > "$TARBALL.sha256" )
STUB

cat > "$WORK/stub-oci.sh" <<STUB
#!/usr/bin/env bash
set -euo pipefail
out="\$1"; cd "\$out"
echo oci-archive-bytes > $OCI.tar
echo "sha256:deadbeef" > $OCI.digest
printf '{"spdxVersion":"SPDX-2.3","packages":[{"name":"a"}]}\n' > $OCI.spdx.json
printf '{"spdxVersion":"SPDX-2.3","packages":[{"name":"b"}]}\n' > $ARCH-cargo.spdx.json
printf '{"bomFormat":"CycloneDX","components":[{"name":"a"}]}\n' > $OCI.cdx.json
printf '{"total_packages":3,"surrealdb_status":{"present_in_cargo_toml":true}}\n' > $OCI-licenses.json
sha() { sha256sum "\$1" | cut -d' ' -f1; }
python3 -I - <<PY
import json, hashlib, os
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
subjects = [{"name": n, "digest": {"sha256": sha(n)}} for n in ("$TARBALL", "$OCI.tar")]
json.dump({"_type": "https://in-toto.io/Statement/v1", "subject": subjects,
           "predicateType": "https://slsa.dev/provenance/v1", "predicate": {}},
          open("$ARCH.provenance.json", "w"))
ref = lambda n: {"filename": n, "sha256": sha(n)}
json.dump({"name": "$BIN", "version": "$VERSION", "commit": "$COMMIT", "arch": "$ARCH",
           "source_date_epoch": 0,
           "tarball": ref("$TARBALL"),
           "oci": {"image": "x", "digest": "sha256:deadbeef", "archive": "$OCI.tar",
                   "sha256": sha("$OCI.tar")},
           "sbom": {"oci_spdx": ref("$OCI.spdx.json"), "oci_cyclonedx": ref("$OCI.cdx.json"),
                    "cargo_spdx": ref("$ARCH-cargo.spdx.json")},
           "license_matrix": ref("$OCI-licenses.json"),
           "provenance": ref("$ARCH.provenance.json"),
           "signing": {"mechanism": "openpgp-detached", "key_mode": os.environ["RELEASE_SIGNING_KEY_MODE"],
                       "public_key": "release-signing-key.asc",
                       "fingerprint": os.environ["RELEASE_GPG_KEY_ID"],
                       "honest_limits": "stub"}},
          open("$MANIFEST", "w"))
PY
GNUPGHOME="\$RELEASE_GPG_HOME" gpg --armor --export "\$RELEASE_GPG_KEY_ID" > release-signing-key.asc
sha256sum $TARBALL $OCI.tar $OCI.digest $OCI.spdx.json $OCI.cdx.json $ARCH-cargo.spdx.json \
  $OCI-licenses.json $ARCH.provenance.json $MANIFEST release-signing-key.asc > SHA256SUMS
GNUPGHOME="\$RELEASE_GPG_HOME" gpg --batch --yes --armor --detach-sign -u "\$RELEASE_GPG_KEY_ID" -o SHA256SUMS.asc SHA256SUMS
GNUPGHOME="\$RELEASE_GPG_HOME" gpg --batch --yes --armor --detach-sign -u "\$RELEASE_GPG_KEY_ID" -o $MANIFEST.asc $MANIFEST
STUB
chmod +x "$WORK/stub-tarball.sh" "$WORK/stub-oci.sh"

export RELEASE_BUNDLE_TARBALL_CMD="$WORK/stub-tarball.sh"
export RELEASE_BUNDLE_OCI_CMD="$WORK/stub-oci.sh"

DIST="$WORK/dist"
"$BUNDLE_SH" "$DIST" > "$WORK/bundle.log" 2>&1 || { cat "$WORK/bundle.log" >&2; fail "release-bundle.sh failed"; }

# 1. Single command produces every component, correlated.
for f in "$TARBALL" "$OCI.tar" "$OCI.digest" "$OCI.spdx.json" "$OCI.cdx.json" "$ARCH-cargo.spdx.json" \
  "$ARCH.provenance.json" "$MANIFEST" "$BIN.rb" "release-notes-$VERSION.md" release-bundle.json \
  release-bundle.json.asc SHA256SUMS SHA256SUMS.asc release-signing-key.asc; do
  [ -f "$DIST/$f" ] || fail "missing bundle asset: $f"
done
for f in "$BIN.rb" "release-notes-$VERSION.md" release-bundle.json; do
  grep -q " $f\$" "$DIST/SHA256SUMS" || fail "SHA256SUMS does not cover $f"
done
grep -q 'class HodeiArtifactsApi < Formula' "$DIST/$BIN.rb" || fail "formula content"
ok "one run emits tarball, OCI, SBOMs, provenance, formula, notes, checksums and signatures"

# 2. Secondary architecture is NOT_RUN with a reason; bundle is partial, not complete.
python3 -I - "$DIST/release-bundle.json" "$ARCH" <<'PY' || fail "bundle manifest status accounting"
import json, sys
b = json.load(open(sys.argv[1])); native = sys.argv[2]
other = "aarch64" if native == "x86_64" else "x86_64"
assert b["status"] == "partial", b["status"]
for comp in ("oci", "sbom", "provenance"):
    c = next(c for c in b["components"] if c["component"] == comp and c["arch"] == other)
    assert c["status"] == "NOT_RUN" and len(c["reason"]) > 20 and "files" not in c, c
    c = next(c for c in b["components"] if c["component"] == comp and c["arch"] == native)
    assert c["status"] == "ok" and c["files"], c
PY
ok "unavailable architecture is recorded as NOT_RUN with a reason (bundle partial, never complete)"

# 3. Clean-directory verification of the whole bundle.
out="$("$VERIFY" --require-bundle "$DIST")" || fail "verify-release.sh rejected a valid bundle"
echo "$out" | grep -q 'status=partial' || fail "verify output must expose the partial status"
echo "$out" | grep -q '^NOT_RUN: oci/' || fail "verify output must repeat NOT_RUN components"
ok "verify-release.sh validates the complete bundle and repeats NOT_RUN"

# 4. Manipulation: every case must be rejected.
expect_reject() { # <label> <mutator-function>
  local label="$1" mut="$2" d="$WORK/t-${1// /_}"
  rm -rf "$d"; cp -r "$DIST" "$d"
  "$mut" "$d"
  if "$VERIFY" --require-bundle "$d" >/dev/null 2>&1; then fail "accepted: $label"; fi
  ok "rejects $label"
}
append() { printf '\n# tamper\n' >> "$1"; }
m_tarball() { append "$1/$TARBALL"; }
m_oci() { append "$1/$OCI.tar"; }
m_sbom() { append "$1/$OCI.spdx.json"; }
m_prov() { append "$1/$ARCH.provenance.json"; }
m_formula() { append "$1/$BIN.rb"; }
m_notes() { append "$1/release-notes-$VERSION.md"; }
m_bundle() { append "$1/release-bundle.json"; }
m_sums() { append "$1/SHA256SUMS"; }
m_nosig() { rm "$1/release-bundle.json.asc"; }
m_nobundle() { rm "$1/release-bundle.json"; }
m_key() { append "$1/release-signing-key.asc"; }
expect_reject "tampered tarball" m_tarball
expect_reject "tampered OCI archive" m_oci
expect_reject "tampered SBOM" m_sbom
expect_reject "tampered provenance" m_prov
expect_reject "tampered formula" m_formula
expect_reject "tampered release notes" m_notes
expect_reject "tampered bundle manifest" m_bundle
expect_reject "tampered SHA256SUMS" m_sums
expect_reject "missing bundle signature" m_nosig
expect_reject "missing bundle manifest" m_nobundle
expect_reject "tampered public key" m_key

# 5. A NOT_RUN without reason or a false-green claim is rejected even when the
#    forger re-signs everything with a key of their own (logic checks, not only
#    cryptography).
build_forgery() { # <dir> <python-mutation of b (bundle) and mf (arch manifest)>
  local d="$1" py="$2"
  rm -rf "$d"; cp -r "$DIST" "$d"
  export GNUPGHOME="$WORK/forger"; rm -rf "$GNUPGHOME"; mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
  gpg --batch --quiet --passphrase '' --quick-generate-key "forger <f@example.invalid>" ed25519 sign never 2>/dev/null
  gpg --batch --quiet --armor --export > "$d/release-signing-key.asc"
  FORGER_FPR="$(gpg --with-colons --list-secret-keys | awk -F: '/^fpr:/ {print $10; exit}')"
  export FORGER_FPR
  python3 -I - "$d" "$py" "$MANIFEST" <<'PY'
import hashlib, json, os, sys
d = sys.argv[1]; p = os.path.join(d, "release-bundle.json"); mp = os.path.join(d, sys.argv[3])
b = json.load(open(p)); mf = json.load(open(mp)); fpr = os.environ["FORGER_FPR"]
exec(sys.argv[2])
# The forger also keeps the key reference consistent with the swapped key.
key = next(c for c in b["components"] if c["component"] == "signing")["files"][0]
key["sha256"] = hashlib.sha256(open(os.path.join(d, "release-signing-key.asc"), "rb").read()).hexdigest()
json.dump(b, open(p, "w")); json.dump(mf, open(mp, "w"))
PY
  ( cd "$d" && mapfile -t names < <(awk '{print $2}' SHA256SUMS) \
    && sha256sum "${names[@]}" > SHA256SUMS.new && mv SHA256SUMS.new SHA256SUMS )
  gpg --batch --quiet --yes --armor --detach-sign -o "$d/SHA256SUMS.asc" "$d/SHA256SUMS"
  gpg --batch --quiet --yes --armor --detach-sign -o "$d/release-bundle.json.asc" "$d/release-bundle.json"
  gpg --batch --quiet --yes --armor --detach-sign -o "$d/$MANIFEST.asc" "$d/$MANIFEST"
  unset GNUPGHOME
}
forge() { # <label> <python-mutation> <expected-message-fragment>
  local label="$1" py="$2" frag="$3" out d="$WORK/f-${1// /_}"
  build_forgery "$d" "$py"
  if out="$("$VERIFY" --require-bundle "$d" 2>&1)"; then fail "accepted forged bundle: $label"; fi
  echo "$out" | grep -q "$frag" || fail "forged $label rejected for the wrong reason: $out"
  ok "rejects re-signed bundle: $label"
}
forge "NOT_RUN without reason" 'next(c for c in b["components"] if c["status"]=="NOT_RUN")["reason"]=""' "without an explicit reason"
forge "complete claimed with NOT_RUN" 'b["status"]="complete"' "claims complete"
forge "unknown status value" 'next(c for c in b["components"] if c["status"]=="NOT_RUN")["status"]="SKIPPED"' "unknown status"
forge "native OCI downgraded to NOT_RUN" 'c=next(c for c in b["components"] if c["component"]=="oci" and c["status"]=="ok"); c["status"]="NOT_RUN"; c["reason"]="x"; del c["files"]' "no architecture with a built OCI"

# 6. Signing identity (HSP-267). Without RELEASE_GPG_* the bundle is signed with
#    an ephemeral key: the per-architecture manifest must say so, in agreement
#    with the bundle, and never claim an operator keyring.
python3 -I - "$DIST" "$MANIFEST" <<'PY' || fail "key_mode of the ephemeral release"
import json, os, sys
d = sys.argv[1]
b = json.load(open(os.path.join(d, "release-bundle.json")))["signing"]
m = json.load(open(os.path.join(d, sys.argv[2])))["signing"]
assert b["key_mode"] == "ephemeral-local", b
for f in ("mechanism", "key_mode", "fingerprint"):
    assert m[f] == b[f], (f, m[f], b[f])
PY
ok "ephemeral release declares key_mode=ephemeral-local in the manifest and the bundle"

forge "key_mode altered only in the bundle" 'b["signing"]["key_mode"]="operator-keyring"' "signing key_mode mismatch"
forge "key_mode altered only in the manifest" 'mf["signing"]["key_mode"]="operator-keyring"' "signing key_mode mismatch"
forge "mechanism altered only in the manifest" 'mf["signing"]["mechanism"]="openpgp-other"' "signing mechanism mismatch"
forge "fingerprint altered only in the bundle" 'b["signing"]["fingerprint"]="ABCDEF"' "signing fingerprint mismatch"

# 7. Pinning the signer: a release re-signed coherently with a foreign key (same
#    fingerprint in manifest and bundle) passes without the option and fails with it.
COHERENT='b["signing"]["fingerprint"]=fpr; mf["signing"]["fingerprint"]=fpr'
build_forgery "$WORK/f-coherent" "$COHERENT"
"$VERIFY" --require-bundle "$WORK/f-coherent" >/dev/null 2>&1 \
  || fail "coherent foreign-key release should pass without --trusted-fingerprint"
ok "coherent foreign-key release passes without --trusted-fingerprint (integrity only)"
REAL_FPR="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["signing"]["fingerprint"])' "$DIST/release-bundle.json")"
if out="$("$VERIFY" --require-bundle --trusted-fingerprint "$REAL_FPR" "$WORK/f-coherent" 2>&1)"; then
  fail "accepted foreign-key release despite --trusted-fingerprint"
fi
echo "$out" | grep -q "does not match --trusted-fingerprint" || fail "wrong rejection reason: $out"
ok "--trusted-fingerprint rejects a coherent foreign-key release"
"$VERIFY" --require-bundle --trusted-fingerprint "$REAL_FPR" "$DIST" >/dev/null 2>&1 \
  || fail "genuine release rejected with its own fingerprint"
"$VERIFY" --require-bundle --trusted-fingerprint "$(echo "$REAL_FPR" | tr 'A-F' 'a-f')" "$DIST" >/dev/null 2>&1 \
  || fail "fingerprint comparison must be case-insensitive"
ok "--trusted-fingerprint accepts the genuine release"
if "$VERIFY" --require-bundle --trusted-fingerprint ABCD "$DIST" >/dev/null 2>&1; then
  fail "accepted a short --trusted-fingerprint"
fi
ok "--trusted-fingerprint requires a full fingerprint"

echo "all $PASS checks passed"
