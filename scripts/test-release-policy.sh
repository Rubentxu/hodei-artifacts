#!/usr/bin/env bash
# Suite for the release policies of HSP-289 that need no cargo build, podman or
# syft: the licence policy (scripts/release-license-matrix.py), the real
# surrealdb/rocksdb state of the matrix, the clean-tree rule and the
# architecture/target coherence of scripts/package-oci.sh.
#
# Usage: scripts/test-release-policy.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOL="$ROOT/scripts/release-license-matrix.py"
POLICY="$ROOT/scripts/release-license-policy.json"
FIXTURE="$ROOT/tests/fixtures/release/cargo-metadata-min.json"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PASS=0
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { PASS=$((PASS + 1)); echo "ok - $*"; }

gen() { python3 -I "$TOOL" generate --policy "$POLICY" --root-package hodei-artifacts-api "$@"; }

# mutate <label> <python-statements on m (cargo metadata)> -> $WORK/<label>.json
mutate() {
  python3 -I - "$FIXTURE" "$WORK/$1.json" "$2" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
byname = {p["name"]: p for p in m["packages"]}
root = byname["hodei-artifacts-api"]
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
exec(sys.argv[3])
json.dump(m, open(sys.argv[2], "w"))
PY
}
ADD_PKG='
def add(name, lic, link=True, lf=None, source="registry+https://github.com/rust-lang/crates.io-index"):
    p = {"name": name, "version": "1.0.0", "id": f"{source}#{name}@1.0.0", "license": lic,
         "license_file": lf, "source": source, "repository": None, "dependencies": []}
    m["packages"].append(p)
    nodes[p["id"]] = {"id": p["id"], "dependencies": [], "deps": [], "features": []}
    m["resolve"]["nodes"].append(nodes[p["id"]])
    if link:
        nodes[root["id"]]["deps"].append({"name": name, "pkg": p["id"], "dep_kinds": [{"kind": None, "target": None}]})
    return p
'

# 1. A compliant tree passes, including SPDX expressions with OR, AND, WITH and legacy "/".
gen --metadata "$FIXTURE" --out "$WORK/clean.json" 2>"$WORK/err" || { cat "$WORK/err" >&2; fail "compliant fixture rejected"; }
python3 -I "$TOOL" check --matrix "$WORK/clean.json" --policy "$POLICY" >/dev/null || fail "check rejected a compliant matrix"
ok "a compliant dependency tree passes (OR, AND, WITH and legacy slash expressions)"

# 2. UNKNOWN licence fails.
mutate unknown "$ADD_PKG"'add("mystery", None)'
if gen --metadata "$WORK/unknown.json" --out "$WORK/o1.json" 2>"$WORK/err"; then fail "accepted an UNKNOWN licence"; fi
grep -q 'mystery@1.0.0: UNKNOWN licence' "$WORK/err" || fail "wrong reason: $(cat "$WORK/err")"
[ ! -e "$WORK/o1.json" ] || fail "a matrix was written despite the violation"
ok "an UNKNOWN licence fails the release and writes no matrix"

# 3. A licence file without SPDX expression is UNKNOWN too (the former surrealdb BSL case) ...
mutate lfile "$ADD_PKG"'add("bsl-crate", None, lf="LICENSE")'
if gen --metadata "$WORK/lfile.json" --out "$WORK/o2.json" 2>/dev/null; then fail "accepted a licence_file-only package"; fi
ok "a package with only a licence file is UNKNOWN and fails"

# 4. ... unless it is a named, justified exception. The shipped policy carries none
# (surrealdb left the tree with HSP-314 F4), so the mechanism is exercised with a policy copy.
python3 -I - "$POLICY" "$WORK/policy-exc.json" <<'PY'
import json, sys
p = json.load(open(sys.argv[1]))
p["exceptions"] = [{"name": "bsl-crate", "reason": "Business Source License 1.1 (test exception: license_file only, no SPDX expression)"}]
json.dump(p, open(sys.argv[2], "w"))
PY
gen --metadata "$WORK/lfile.json" --out "$WORK/o3.json" --policy "$WORK/policy-exc.json" || fail "a named exception must be honoured"
python3 -I - "$WORK/o3.json" <<'PY' || fail "exception not recorded"
import json, sys
m = json.load(open(sys.argv[1]))
assert "bsl-crate@1.0.0" in m["policy"]["exceptions_used"], m["policy"]
assert any("Business Source License" in e["reason"] for e in m["policy"]["exceptions"])
PY
ok "a justified exception is honoured and recorded in the matrix"

# 5. Outside the allowlist fails, also inside an AND and when every OR alternative is disallowed.
for lic in 'GPL-3.0-only' 'MIT AND GPL-3.0-only' 'GPL-2.0-only OR AGPL-3.0-only' 'Not a licence ((' ; do
  mutate bad "$ADD_PKG"'add("bad", """'"$lic"'""")'
  if gen --metadata "$WORK/bad.json" --out "$WORK/o4.json" 2>"$WORK/err"; then fail "accepted licence: $lic"; fi
  grep -q 'bad@1.0.0' "$WORK/err" || fail "no violation named for: $lic"
done
ok "licences outside the allowlist (alone, in AND, all-OR-disallowed, unparsable) fail"

# 6. An OR with one allowed alternative passes (dual licensed with a copyleft option).
mutate dual "$ADD_PKG"'add("dual", "MIT OR Apache-2.0 OR LGPL-2.1-or-later")'
gen --metadata "$WORK/dual.json" --out "$WORK/o5.json" || fail "a dual licence with an allowed alternative must pass"
ok "an OR expression passes when one alternative is allowed"

# 7. The checker re-evaluates a matrix; an edited one cannot hide a violation.
python3 -I - "$WORK/clean.json" "$WORK/forged.json" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
m["packages"].append({"name": "evil", "version": "1.0.0", "source": "registry", "license": "GPL-3.0-only"})
m["policy"]["violations"] = []
json.dump(m, open(sys.argv[2], "w"))
PY
if python3 -I "$TOOL" check --matrix "$WORK/forged.json" --policy "$POLICY" >/dev/null 2>&1; then fail "check accepted a forged matrix"; fi
python3 -I - "$WORK/clean.json" "$WORK/forged2.json" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
m["policy"]["allowed"].append("GPL-3.0-only")
json.dump(m, open(sys.argv[2], "w"))
PY
if python3 -I "$TOOL" check --matrix "$WORK/forged2.json" --policy "$POLICY" >/dev/null 2>&1; then fail "check accepted a widened allowlist"; fi
ok "check re-evaluates the packages and rejects a widened allowlist"

# 8. The surrealdb/rocksdb state is the real one, never a pinned constant.
mutate surreal "$ADD_PKG"'
s = add("surrealdb", "Apache-2.0")
r = add("surrealdb-rocksdb", "Apache-2.0")
root["dependencies"].append({"name": "surrealdb"})
'
cat > "$WORK/cdx-clean.json" <<'JSON'
{"bomFormat": "CycloneDX", "components": [{"name": "libc6"}]}
JSON
cat > "$WORK/cdx-surreal.json" <<'JSON'
{"bomFormat": "CycloneDX", "components": [{"name": "libc6"}, {"name": "surrealdb", "purl": "pkg:cargo/surrealdb@2.7.0"}]}
JSON
gen --metadata "$FIXTURE" --out "$WORK/s0.json" --oci-cdx "$WORK/cdx-clean.json"
gen --metadata "$WORK/surreal.json" --out "$WORK/s1.json" --oci-cdx "$WORK/cdx-clean.json"
python3 -I - "$WORK/s0.json" "$WORK/s1.json" <<'PY' || fail "surrealdb status is not the real one"
import json, sys
absent = json.load(open(sys.argv[1]))["surrealdb_status"]
linked = json.load(open(sys.argv[2]))["surrealdb_status"]
assert absent["present_in_cargo_toml"] is False and absent["present_in_resolved_graph"] is False, absent
assert absent["oci_sbom_exclusion"]["status"] == "ok", absent
assert linked["present_in_cargo_toml"] is True and linked["present_in_resolved_graph"] is True, linked
assert linked["rocksdb_in_resolved_graph"] is True and "surrealdb" in linked["reachable_from_release_binary"], linked
# The binary links surrealdb: the SBOM exclusion is NOT_RUN with a reason, never "ok".
assert linked["oci_sbom_exclusion"]["status"] == "NOT_RUN" and linked["oci_sbom_exclusion"]["reason"], linked
PY
ok "present_in_cargo_toml/resolved graph follow the real tree; a linked surrealdb makes the SBOM exclusion NOT_RUN"
python3 -I "$TOOL" check --matrix "$WORK/s1.json" --policy "$POLICY" --oci-cdx "$WORK/cdx-clean.json" | grep -q '^NOT_RUN: oci-sbom-exclusion' \
  || fail "check must repeat the NOT_RUN of the SBOM exclusion"
ok "check repeats the NOT_RUN of the SBOM exclusion"

# 9. When the binary does not link them, the SBOM must not list them.
if python3 -I "$TOOL" check --matrix "$WORK/s0.json" --policy "$POLICY" --oci-cdx "$WORK/cdx-surreal.json" >/dev/null 2>&1; then
  fail "accepted an SBOM listing surrealdb for a binary that does not link it"
fi
if gen --metadata "$FIXTURE" --out "$WORK/s2.json" --oci-cdx "$WORK/cdx-surreal.json" 2>/dev/null; then
  fail "generate must fail when the SBOM lists surrealdb for an unlinked binary"
fi
ok "an OCI SBOM that lists surrealdb contradicts an unlinked binary"

# 10. The real tree of this checkout: the policy holds today and the matrix tells the truth.
if command -v cargo >/dev/null 2>&1 && cargo metadata --offline --locked --format-version 1 > "$WORK/real-meta.json" 2>/dev/null; then
  gen --metadata "$WORK/real-meta.json" --out "$WORK/real.json" 2>"$WORK/err" || { cat "$WORK/err" >&2; fail "the real dependency tree violates the licence policy"; }
  python3 -I - "$WORK/real.json" "$ROOT/Cargo.toml" <<'PY' || fail "matrix disagrees with Cargo.toml"
import json, re, sys
st = json.load(open(sys.argv[1]))["surrealdb_status"]
toml = open(sys.argv[2], encoding="utf-8").read()
declared = re.search(r"^surrealdb\s*=", toml, re.M) is not None
assert st["present_in_cargo_toml"] is declared, (st["present_in_cargo_toml"], declared)
assert st["present_in_resolved_graph"] is bool(st["versions"])
PY
  ok "the licence policy holds on the real tree and surrealdb_status matches Cargo.toml"
else
  echo "NOT_RUN: real-tree licence policy: cargo metadata --offline --locked unavailable (dependencies not vendored)"
fi

# 11. Clean-tree rule (provenance tree_clean) and architecture/target coherence.
. "$ROOT/scripts/lib-release-tree.sh"
REPO="$WORK/repo"
mkdir -p "$REPO/scripts"
cp "$ROOT/scripts/package-oci.sh" "$ROOT/scripts/lib-release-tree.sh" "$REPO/scripts/"
printf '[package]\nname = "hodei-artifacts-api"\nversion = "0.0.1"\n' > "$REPO/Cargo.toml"
git -C "$REPO" init -q
git -C "$REPO" config core.hooksPath /dev/null
# The fixture repository must not depend on the commit policies of the machine
# running the suite: the commit object is created with plumbing and the branch
# ref is written directly.
git -C "$REPO" add -A
TREE="$(git -C "$REPO" write-tree)"
COMMIT="$(GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid \
  git -C "$REPO" commit-tree "$TREE" -m init)"
BRANCH_REF="$(git -C "$REPO" symbolic-ref HEAD)"
mkdir -p "$REPO/.git/$(dirname "$BRANCH_REF")"
echo "$COMMIT" > "$REPO/.git/$BRANCH_REF"
[ "$(release_tree_clean "$REPO" "$REPO/dist")" = true ] || fail "a committed tree must be clean"
ok "a committed tree reports tree_clean=true"
mkdir -p "$REPO/dist" && echo artifact > "$REPO/dist/a.tar"
[ "$(release_tree_clean "$REPO" "$REPO/dist")" = true ] || fail "the output directory must not make the tree dirty"
ok "the release output directory does not dirty the tree"
echo change >> "$REPO/Cargo.toml"
[ "$(release_tree_clean "$REPO" "$REPO/dist")" = false ] || fail "a modified file must make the tree dirty"
out="$(cd "$REPO" && env -u HODEI_ALLOW_DIRTY_TREE scripts/package-oci.sh "$REPO/dist" 2>&1)" && fail "package-oci.sh accepted a dirty tree"
echo "$out" | grep -q 'working tree is dirty' || fail "wrong rejection: $out"
ok "package-oci.sh fails on a dirty tree before any build"
git -C "$REPO" checkout -q -- Cargo.toml
echo scratch > "$REPO/untracked.rs"
[ "$(release_tree_clean "$REPO" "$REPO/dist")" = false ] || fail "an untracked source file must make the tree dirty"
ok "an untracked file dirties the tree"
rm "$REPO/untracked.rs"
out="$(cd "$REPO" && HODEI_ALLOW_DIRTY_TREE=1 release_require_clean_tree "$REPO" "$REPO/dist")"
[ "$out" = true ] || fail "a clean tree must report true even with the override"
echo change >> "$REPO/Cargo.toml"
out="$(HODEI_ALLOW_DIRTY_TREE=1 release_require_clean_tree "$REPO" "$REPO/dist")"
[ "$out" = false ] || fail "the dirty override must still record false"
ok "HODEI_ALLOW_DIRTY_TREE=1 builds on but records tree_clean=false"
git -C "$REPO" checkout -q -- Cargo.toml

out="$(cd "$REPO" && HODEI_OCI_ARCH=x86_64 OCI_RUST_TARGET=aarch64-unknown-linux-gnu.2.36 scripts/package-oci.sh "$REPO/dist" 2>&1)" \
  && fail "package-oci.sh accepted an aarch64 target for an x86_64 release"
echo "$out" | grep -q 'targets aarch64 but the release architecture is x86_64' || fail "wrong rejection: $out"
out="$(cd "$REPO" && HODEI_OCI_ARCH=aarch64 OCI_RUST_TARGET=x86_64-unknown-linux-gnu scripts/package-oci.sh "$REPO/dist" 2>&1)" \
  && fail "package-oci.sh accepted an x86_64 target for an aarch64 release"
echo "$out" | grep -q 'targets x86_64 but the release architecture is aarch64' || fail "wrong rejection: $out"
ok "package-oci.sh rejects an OCI_RUST_TARGET whose architecture differs from the release architecture"

echo "all $PASS checks passed"
