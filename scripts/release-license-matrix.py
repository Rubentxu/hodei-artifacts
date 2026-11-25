#!/usr/bin/env python3
"""Dependency/licence matrix of a release and its licence policy (HSP-289).

  release-license-matrix.py generate --metadata <cargo-metadata.json> --out <matrix.json>
                                     --policy <policy.json> --root-package <name>
                                     [--oci-cdx <cyclonedx.json>]
  release-license-matrix.py check    --matrix <matrix.json> --policy <policy.json>
                                     [--oci-cdx <cyclonedx.json>]

`generate` writes the matrix and exits non-zero when any package is UNKNOWN or
outside the allowlist (and is not a named exception), so a release cannot be
produced from a tree that violates the policy. `check` re-evaluates a matrix
from its own package list (a forged `violations` field cannot hide anything),
rejects an allowlist that differs from the repository policy, and checks that
the surrealdb/rocksdb exclusion claim agrees with the OCI SBOM.
"""

import argparse
import collections
import hashlib
import json
import re
import sys

STORAGE_ENGINES = ("surrealdb", "rocksdb")
FIRST_PARTY = "UNDECLARED-FIRST-PARTY"


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(1)


def load(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def file_sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


# ---- SPDX expression evaluation -------------------------------------------

TOKEN = re.compile(r"\(|\)|[^\s()]+")


def satisfied(expression, allowed):
    """True when the SPDX expression is satisfiable with the allowed ids.

    `A OR B`: any alternative; `A AND B`: every term; `A WITH E`: allowed when
    `A WITH E` or `A` is. A legacy `A/B` is read as `A OR B`.
    """
    tokens = []
    for tok in TOKEN.findall(expression.replace("/", " OR ")):
        tokens.append(tok)
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else None

    def take():
        nonlocal pos
        tok = peek()
        pos += 1
        return tok

    def parse_or():
        result = parse_and()
        while peek() == "OR":
            take()
            right = parse_and()
            result = result or right
        return result

    def parse_and():
        result = parse_atom()
        while peek() == "AND":
            take()
            right = parse_atom()
            result = result and right
        return result

    def parse_atom():
        tok = take()
        if tok is None:
            raise ValueError("unexpected end of expression")
        if tok == "(":
            result = parse_or()
            if take() != ")":
                raise ValueError("missing closing parenthesis")
            return result
        if tok in ("AND", "OR", "WITH", ")"):
            raise ValueError(f"unexpected token {tok!r}")
        ident = tok
        if peek() == "WITH":
            take()
            exc = take()
            if exc is None:
                raise ValueError("WITH without exception")
            return f"{ident} WITH {exc}" in allowed or ident in allowed
        return ident in allowed

    result = parse_or()
    if pos != len(tokens):
        raise ValueError(f"trailing token {tokens[pos]!r}")
    return result


def evaluate(pkg, policy, allowed):
    """Return None when the package passes, else the reason it does not."""
    lic = pkg["license"]
    exceptions = {e["name"] for e in policy.get("exceptions", [])}
    if lic == FIRST_PARTY:
        if policy.get("first_party_undeclared_allowed") and pkg.get("source") == "workspace":
            return None
        return "first-party package without a declared licence"
    if lic == "UNKNOWN":
        reason = "UNKNOWN licence"
    else:
        try:
            if satisfied(lic, allowed):
                return None
        except ValueError as err:
            return f"unparsable licence expression {lic!r}: {err}"
        reason = f"licence {lic!r} outside the allowlist"
    if pkg["name"] in exceptions:
        return None
    return reason


# ---- Dependency graph -------------------------------------------------------


def reachable_names(meta, root_name):
    """Names of the packages linked into `root_name` through normal dependencies."""
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta.get("resolve", {}).get("nodes", [])}
    roots = [i for i, p in by_id.items() if p["name"] == root_name and p.get("source") is None]
    if not roots:
        die(f"root package {root_name!r} not found in cargo metadata")
    seen, stack = set(), list(roots)
    while stack:
        cur = stack.pop()
        if cur in seen:
            continue
        seen.add(cur)
        for dep in nodes.get(cur, {}).get("deps", []):
            if any(k.get("kind") is None for k in dep.get("dep_kinds", [])):
                stack.append(dep["pkg"])
    return {by_id[i]["name"] for i in seen if i in by_id}


def sbom_mentions(cdx_path):
    cdx = load(cdx_path)
    names = set()
    for comp in cdx.get("components", []):
        text = f"{comp.get('name', '')} {comp.get('purl', '')}".lower()
        for engine in STORAGE_ENGINES:
            if engine in text:
                names.add(engine)
    return sorted(names)


def exclusion_check(reachable, cdx_path):
    """The claim 'the OCI SBOM carries neither surrealdb nor rocksdb' made honest."""
    linked = sorted(n for n in reachable if any(e in n for e in STORAGE_ENGINES))
    if linked:
        return {
            "status": "NOT_RUN",
            "reason": (
                "the release binary links " + ", ".join(linked) + " through normal dependencies, so "
                "an SBOM free of them cannot be claimed; it becomes checkable once the removal "
                "(HSP-213) is integrated"
            ),
            "mentioned_in_oci_sbom": sbom_mentions(cdx_path) if cdx_path else None,
        }
    if not cdx_path:
        return {"status": "NOT_RUN", "reason": "no OCI CycloneDX SBOM was provided to inspect",
                "mentioned_in_oci_sbom": None}
    mentioned = sbom_mentions(cdx_path)
    return {
        "status": "ok" if not mentioned else "violation",
        "reason": "OCI SBOM inspected for surrealdb and rocksdb components",
        "mentioned_in_oci_sbom": mentioned,
    }


# ---- generate ---------------------------------------------------------------


def generate(args):
    policy = load(args.policy)
    allowed = set(policy["allowed"])
    meta = load(args.metadata)
    root = next((p for p in meta["packages"] if p["name"] == args.root_package and p.get("source") is None), None)
    if root is None:
        die(f"root package {args.root_package!r} not found in cargo metadata")
    reachable = reachable_names(meta, args.root_package)

    packages, counts, unlicensed = [], collections.Counter(), []
    surreal_versions, declared_by = [], []
    for pkg in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        source = pkg.get("source") or "workspace"
        lic = pkg.get("license")
        entry = {"name": pkg["name"], "version": pkg["version"], "source": source,
                 "repository": pkg.get("repository")}
        if lic:
            entry["license"] = lic
        elif source == "workspace" and not pkg.get("license_file"):
            entry["license"] = FIRST_PARTY
        else:
            entry["license"] = "UNKNOWN"
            if pkg.get("license_file"):
                entry["license_file"] = pkg["license_file"]
        if entry["license"] == "UNKNOWN":
            unlicensed.append(f"{pkg['name']}@{pkg['version']}")
        counts[entry["license"]] += 1
        if pkg["name"] == "surrealdb":
            surreal_versions.append(pkg["version"])
        if source == "workspace" and any(d["name"] == "surrealdb" for d in pkg.get("dependencies", [])):
            declared_by.append(pkg["name"])
        packages.append(entry)

    violations, exceptions_used = [], []
    for entry in packages:
        reason = evaluate(entry, policy, allowed)
        if reason:
            violations.append({"package": f"{entry['name']}@{entry['version']}", "reason": reason})
        elif entry["license"] == "UNKNOWN" or (
            entry["name"] in {e["name"] for e in policy.get("exceptions", [])}
            and not _passes_plainly(entry, allowed)
        ):
            exceptions_used.append(f"{entry['name']}@{entry['version']}")

    present_in_toml = any(d["name"] == "surrealdb" for d in root.get("dependencies", []))
    in_graph = bool(surreal_versions)
    rocksdb_in_graph = any("rocksdb" in p["name"] for p in meta["packages"])
    check = exclusion_check(reachable, args.oci_cdx)
    if in_graph:
        note = (
            "surrealdb is a real dependency of this checkout (resolved graph; the workspace crates "
            "listed in declared_by use it) and HSP-213, its removal, is not integrated here; "
            "this matrix records the resolved state and does not assert the removal."
        )
    else:
        note = "surrealdb is absent from the resolved dependency graph (HSP-213 integrated)."

    matrix = {
        "schema_version": 2,
        "generator": "cargo metadata --locked --format-version 1",
        "total_packages": len(packages),
        "license_summary": dict(sorted(counts.items())),
        "unlicensed_packages": unlicensed,
        "surrealdb_status": {
            "present_in_cargo_toml": present_in_toml,
            "declared_by": sorted(declared_by),
            "present_in_resolved_graph": in_graph,
            "rocksdb_in_resolved_graph": rocksdb_in_graph,
            "reachable_from_release_binary": sorted(
                n for n in reachable if any(e in n for e in STORAGE_ENGINES)),
            "versions": surreal_versions,
            "oci_sbom_exclusion": check,
            "note": note,
        },
        "policy": {
            "sha256": file_sha256(args.policy),
            "allowed": sorted(allowed),
            "first_party_undeclared_allowed": bool(policy.get("first_party_undeclared_allowed")),
            "exceptions": policy.get("exceptions", []),
            "exceptions_used": exceptions_used,
            "violations": violations,
        },
        "packages": packages,
    }
    if check["status"] == "violation":
        die("the OCI SBOM lists " + ", ".join(check["mentioned_in_oci_sbom"])
            + " although the release binary does not link them")
    if violations:
        for v in violations:
            print(f"licence policy violation: {v['package']}: {v['reason']}", file=sys.stderr)
        die(f"{len(violations)} package(s) violate the licence policy")
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(matrix, f, indent=2)
        f.write("\n")


def _passes_plainly(entry, allowed):
    if entry["license"] in ("UNKNOWN", FIRST_PARTY):
        return False
    try:
        return satisfied(entry["license"], allowed)
    except ValueError:
        return False


# ---- check ------------------------------------------------------------------


def check(args):
    matrix = load(args.matrix)
    repo_policy = load(args.policy)
    pol = matrix.get("policy")
    if not isinstance(pol, dict) or not matrix.get("packages"):
        die("license_matrix carries no policy section or packages")
    if pol.get("sha256") != file_sha256(args.policy):
        die("license_matrix was produced under a different licence policy than the repository one")
    if set(pol.get("allowed", [])) != set(repo_policy["allowed"]):
        die("license_matrix allowlist differs from the repository licence policy")
    effective = {"exceptions": repo_policy.get("exceptions", []),
                 "first_party_undeclared_allowed": repo_policy.get("first_party_undeclared_allowed")}
    allowed = set(repo_policy["allowed"])
    bad = []
    for entry in matrix["packages"]:
        reason = evaluate(entry, effective, allowed)
        if reason:
            bad.append(f"{entry['name']}@{entry['version']}: {reason}")
    if bad:
        die("licence policy violated: " + "; ".join(bad))
    if matrix.get("unlicensed_packages") and not all(
        u.split("@")[0] in {e["name"] for e in repo_policy.get("exceptions", [])}
        for u in matrix["unlicensed_packages"]
    ):
        die("licence matrix lists unlicensed packages without a justified exception")

    st = matrix.get("surrealdb_status", {})
    excl = st.get("oci_sbom_exclusion")
    if not isinstance(excl, dict):
        die("license_matrix lacks surrealdb_status.oci_sbom_exclusion")
    linked = st.get("reachable_from_release_binary")
    if linked is None:
        die("license_matrix lacks surrealdb_status.reachable_from_release_binary")
    if linked and excl.get("status") != "NOT_RUN":
        die("the release binary links surrealdb/rocksdb but the matrix claims an SBOM exclusion")
    if not linked and args.oci_cdx:
        mentioned = sbom_mentions(args.oci_cdx)
        if mentioned:
            die("OCI SBOM lists " + ", ".join(mentioned) + " although the binary does not link them")
        if excl.get("status") != "ok":
            die("OCI SBOM exclusion is not recorded as ok although it holds")
    if excl.get("status") == "NOT_RUN" and not str(excl.get("reason", "")).strip():
        die("OCI SBOM exclusion is NOT_RUN without a reason")
    if excl.get("status") == "NOT_RUN":
        print(f"NOT_RUN: oci-sbom-exclusion: {excl['reason']}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("generate")
    g.add_argument("--metadata", required=True)
    g.add_argument("--out", required=True)
    g.add_argument("--policy", required=True)
    g.add_argument("--root-package", required=True)
    g.add_argument("--oci-cdx")
    c = sub.add_parser("check")
    c.add_argument("--matrix", required=True)
    c.add_argument("--policy", required=True)
    c.add_argument("--oci-cdx")
    args = ap.parse_args()
    generate(args) if args.cmd == "generate" else check(args)


if __name__ == "__main__":
    main()
