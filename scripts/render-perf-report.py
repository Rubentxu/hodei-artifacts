#!/usr/bin/env python3
"""Render a human-readable Spanish report from a perf-lab JSON run.

Plan item 6 of HSP-140: raw JSON plus a legible report kept apart. This
reads target/uat/perf-lab-*.json and prints a markdown summary: meta,
host, per-workload aggregate, budget table, failures and declared skips.
"""
import json
import sys


def fmt(v, nd=2, suffix=""):
    if v is None:
        return "NO MEDIDO"
    return f"{v:,.{nd}f}{suffix}"


def main():
    src, dst = sys.argv[1], sys.argv[2]
    d = json.load(open(src))
    meta = d["meta"]
    host = d.get("host_state", {})
    out = []
    out.append(f"# Informe de rendimiento PERF-01 — perfil `{meta['profile']}`")
    out.append("")
    out.append(f"- Herramienta: `{meta['tool']}` v{meta['version']} · commit `{meta['commit']}`")
    out.append(f"- Comando: `{meta['command']}`")
    out.append(f"- Inicio: {meta['started_at']} · estado del informe: `{meta.get('report_state','complete')}` "
               f"({meta['replicas_completed']}/{meta['replicas_requested']} réplicas)")
    out.append(f"- Anfitrión: {host.get('nproc','?')} núcleos, governor `{host.get('cpu_governor','?')}`, "
               f"RAM {host.get('mem_total','?')} kB, kernel {host.get('kernel','?')}")
    out.append("")
    out.append("## Siembra medida")
    for r in d["replicas"]:
        s = r["seeds"]
        out.append(f"- Réplica {r['replica']}: {s['catalog_assets']:,} assets en {fmt(s['catalog_bulk_seconds'],1)} s; "
                   f"{s['evidence']:,} evidencias en {fmt(s['evidence_seconds'],1)} s")
    out.append("")
    out.append("## Workloads (agregado de réplicas)")
    out.append("")
    out.append("| workload | p50 ms | p95 ms | p99 ms | rps | errores | timeouts |")
    out.append("| --- | --: | --: | --: | --: | --: | --: |")
    agg = d.get("aggregate", {})
    for k in sorted(agg):
        v = agg[k]
        p99 = fmt(v["p99_ms_median"])
        out.append(f"| {k} | {fmt(v['p50_ms_median'])} | {fmt(v['p95_ms_median'])} | {p99} | "
                   f"{fmt(v['throughput_rps_median'],3)} | {v['errors_total']} | {v['timeouts_total']} |")
    out.append("")
    if agg:
        first = next(iter(agg.values()))
        if "peak_rss_kib_max" in first:
            pass
    out.append("## Sonda de permisos por réplica")
    for r in d["replicas"]:
        p = r.get("permission_probe", {})
        cases = ", ".join(f"{c}: {v.get('status')}/{v.get('expected')}{' OK' if v.get('pass') else ' FALLO'}"
                          for c, v in p.items())
        out.append(f"- Réplica {r['replica']}: {cases}")
    out.append("")
    b = d.get("baseline_and_budget")
    if b:
        out.append("## Baseline y presupuesto de regresión")
        out.append("")
        o = b["observed"]
        out.append(f"- Peor spread entre réplicas: p99 {fmt(o['worst_p99_replica_spread_pct'])} %, "
                   f"throughput {fmt(o['worst_throughput_replica_spread_pct'])} %.")
        out.append(f"- Propuesta: p99 +{b['proposal']['p99_increase_pct']} %, throughput −{b['proposal']['throughput_drop_pct']} %.")
        nf = [k for k, v in b["per_workload"].items() if v["budget_covers_noise"] is False]
        nn = [k for k, v in b["per_workload"].items() if v["budget_covers_noise"] is None]
        cov = [k for k, v in b["per_workload"].items() if v["budget_covers_noise"] is True]
        out.append(f"- La propuesta cubre el ruido medido en {len(cov)}/{len(b['per_workload'])} workloads; "
                   f"no lo cubre en: {', '.join(sorted(nf)) if nf else 'ninguno'}"
                   + (f"; sin datos suficientes: {len(nn)}" if nn else "") + ".")
    out.append("")
    skips = d.get("skips_declared", [])
    if skips:
        out.append("## Exclusiones declaradas (nada inventado)")
        for s in skips:
            out.append(f"- **{s['id']}**: {s['motive']}")
        out.append("")
    fails = d.get("replica_failures", [])
    if fails:
        out.append("## Fallos de réplica")
        for f in fails:
            out.append(f"- {json.dumps(f)}")
    text = "\n".join(out) + "\n"
    open(dst, "w").write(text)
    print(f"informe: {dst}")


if __name__ == "__main__":
    main()
