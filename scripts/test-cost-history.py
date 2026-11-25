#!/usr/bin/env python3
"""Lightweight local test-cost history collector and offline HTML trend renderer.

Reuses existing reports from `scripts/test-affected.sh`, `pipeline.kts`, and
`target/uat/perf-lab-*.json` to produce per-run JSON files under
`target/reports/runs/` and a 100% offline static HTML trend report at
`target/reports/index.html`.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import html
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

SCHEMA = "hodei.test-cost-history/v1"
UNKNOWN = "UNKNOWN"
VALID_PHASES = ("setup", "compile", "link", "execution")
VALID_STATUSES = ("PASSED", "FAILED", "NOT_RUN")

FORBIDDEN_KEYS = frozenset({
    "root",
    "user",
    "username",
    "author",
    "email",
    "hostname",
    "host_name",
    "env",
    "environment",
    "credentials",
    "token",
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "access_key",
    "secret_key",
    "private_key",
    "authorization",
})

REDACTION_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (re.compile(r"/home/[^\s/\"']+"), "/home/<REDACTED>"),
    (re.compile(r"/Users/[^\s/\"']+"), "/Users/<REDACTED>"),
    (re.compile(r"C:\\Users\\[^\s\\\"']+"), r"C:\\Users\\<REDACTED>"),
    (re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"), "<REDACTED_EMAIL>"),
    (re.compile(r"(Bearer\s+)[A-Za-z0-9._\-~+/]+=*", re.IGNORECASE), r"\1<REDACTED>"),
    (re.compile(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}\b"), "<REDACTED_TOKEN>"),
    (re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b"), "<REDACTED_TOKEN>"),
    (re.compile(r"\bsk-[A-Za-z0-9]{20,}\b"), "<REDACTED_TOKEN>"),
    (re.compile(r"\bAKIA[0-9A-Z]{16}\b"), "<REDACTED_TOKEN>"),
    (re.compile(r"\bxox[baprs]-[A-Za-z0-9-]+\b"), "<REDACTED_TOKEN>"),
    (re.compile(r"(https?://)[^:\s/@]+:[^@\s/@]+@"), r"\1<REDACTED>@"),
    (
        re.compile(
            r"(\b[A-Za-z0-9_]*(?:password|passwd|secret|token|api_key|apikey|access_key|secret_key|private_key|authorization)"
            r"\s*[:=]\s*['\"]?)([^\s'\",;]+)(['\"]?)",
            re.IGNORECASE,
        ),
        r"\1<REDACTED>\3",
    ),
)

CARGO_TEST_SUMMARY_RE = re.compile(
    r"test result:\s+(?:ok|FAILED)\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;\s+(\d+)\s+ignored;"
)


def redact_text(value: str) -> str:
    """Redact personal paths, emails, credentials, and tokens from a string."""
    result = value
    for pattern, replacement in REDACTION_PATTERNS:
        result = pattern.sub(replacement, result)
    return result


def sanitize_value(value: Any) -> Any:
    """Recursively strip forbidden PII/secret keys and redact strings."""
    if isinstance(value, dict):
        cleaned: dict[str, Any] = {}
        for key, item in value.items():
            if str(key).lower() in FORBIDDEN_KEYS:
                continue
            cleaned[key] = sanitize_value(item)
        return cleaned
    if isinstance(value, list):
        return [sanitize_value(item) for item in value]
    if isinstance(value, str):
        return redact_text(value)
    return value


def _read_text_optional(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return None


def collect_initial_resources(
    limits: str = UNKNOWN,
    host_override: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Collect non-identifying initial host resources; use UNKNOWN when unavailable."""
    if host_override is not None:
        return {
            "cpu_cores": host_override.get("cpu_cores", host_override.get("nproc", UNKNOWN)) or UNKNOWN,
            "mem_total_kib": host_override.get("mem_total_kib", host_override.get("mem_total", UNKNOWN)) or UNKNOWN,
            "mem_available_kib": host_override.get("mem_available_kib", UNKNOWN) or UNKNOWN,
            "cpu_governor": host_override.get("cpu_governor", UNKNOWN) or UNKNOWN,
            "load_avg_1m": host_override.get("load_avg_1m", UNKNOWN) or UNKNOWN,
            "limits": redact_text(str(limits or UNKNOWN)),
        }

    cpu_cores: int | str = os.cpu_count() or UNKNOWN

    mem_total_kib: int | str = UNKNOWN
    mem_available_kib: int | str = UNKNOWN
    meminfo = _read_text_optional(Path("/proc/meminfo"))
    if meminfo:
        for line in meminfo.splitlines():
            if line.startswith("MemTotal:"):
                parts = line.split()
                if len(parts) >= 2 and parts[1].isdigit():
                    mem_total_kib = int(parts[1])
            elif line.startswith("MemAvailable:"):
                parts = line.split()
                if len(parts) >= 2 and parts[1].isdigit():
                    mem_available_kib = int(parts[1])

    governor_raw = _read_text_optional(
        Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    )
    cpu_governor: str = governor_raw if governor_raw else UNKNOWN

    load_avg_1m: float | str = UNKNOWN
    loadavg_raw = _read_text_optional(Path("/proc/loadavg"))
    if loadavg_raw:
        first = loadavg_raw.split()[0]
        try:
            load_avg_1m = float(first)
        except ValueError:
            load_avg_1m = UNKNOWN

    return {
        "cpu_cores": cpu_cores,
        "mem_total_kib": mem_total_kib,
        "mem_available_kib": mem_available_kib,
        "cpu_governor": cpu_governor,
        "load_avg_1m": load_avg_1m,
        "limits": redact_text(str(limits or UNKNOWN)),
    }


def _normalize_status(raw_status: str) -> str:
    upper = raw_status.strip().upper()
    if upper in ("PASS", "PASSED", "OK", "SUCCESS"):
        return "PASSED"
    if upper in ("FAIL", "FAILED", "ERROR"):
        return "FAILED"
    if upper in ("NOT_RUN", "SKIP", "SKIPPED", "IGNORED"):
        return "NOT_RUN"
    return "NOT_RUN"


def _classify_gate_phase(gate_name: str, kind: str) -> str:
    lower_gate = gate_name.lower()
    lower_kind = kind.lower()
    if lower_kind in ("fmt", "openapi", "setup") or lower_gate.startswith("fmt"):
        return "setup"
    if lower_kind in ("clippy", "build", "compile") or lower_gate.startswith("clippy"):
        return "compile"
    if lower_kind == "link":
        return "link"
    return "execution"


def _resource_or_unknown(value: Any, cast_type: type = int) -> Any:
    if value is None or value == "" or value == UNKNOWN:
        return UNKNOWN
    try:
        return cast_type(value)
    except (TypeError, ValueError):
        return UNKNOWN


def _make_phase(
    name: str,
    stage: str,
    duration_ms: int,
    status: str,
    cpu_user_ms: Any = UNKNOWN,
    cpu_sys_ms: Any = UNKNOWN,
    cpu_pct: Any = UNKNOWN,
    max_rss_kib: Any = UNKNOWN,
) -> dict[str, Any]:
    return {
        "name": name if name in VALID_PHASES else "execution",
        "stage": redact_text(stage),
        "duration_ms": max(0, int(duration_ms)),
        "status": _normalize_status(status),
        "cpu_user_ms": _resource_or_unknown(cpu_user_ms, int),
        "cpu_sys_ms": _resource_or_unknown(cpu_sys_ms, int),
        "cpu_pct": _resource_or_unknown(cpu_pct, float),
        "max_rss_kib": _resource_or_unknown(max_rss_kib, int),
    }


def _git_info_from_cwd() -> dict[str, str]:
    def _run_git(args: list[str]) -> str:
        try:
            proc = subprocess.run(
                ["git", *args],
                capture_output=True,
                text=True,
                check=True,
            )
            return proc.stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            return ""

    sha = _run_git(["rev-parse", "HEAD"]) or "0000000"
    branch = _run_git(["rev-parse", "--abbrev-ref", "HEAD"]) or "HEAD"
    return {
        "sha": sha,
        "short": sha[:7],
        "branch": branch,
        "base_commit": "",
    }


def _parse_cargo_log_counts(log_path: Path) -> tuple[int, int, int, list[str]]:
    if not log_path.is_file():
        return (0, 0, 0, [])
    try:
        text = log_path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return (0, 0, 0, [])

    passed = 0
    failed = 0
    ignored = 0
    for match in CARGO_TEST_SUMMARY_RE.finditer(text):
        passed += int(match.group(1))
        failed += int(match.group(2))
        ignored += int(match.group(3))

    failure_lines = [
        redact_text(line.strip())
        for line in text.splitlines()
        if line.strip().startswith("error") or "FAILED" in line or "panicked at" in line
    ][:10]
    return (passed, failed, ignored, failure_lines)


def build_run_from_test_affected(
    report_path: Path,
    logs_dir: Path | None = None,
    recorded_at: str | None = None,
    initial_resources: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Convert a `scripts/test-affected.py` report JSON into `hodei.test-cost-history/v1`."""
    data = json.loads(report_path.read_text(encoding="utf-8"))
    repo = data.get("repository", {})
    base = repo.get("base", {})
    sha = str(repo.get("commit") or "0000000")
    crates = [str(c.get("name", "")) for c in data.get("affected_crates", []) if c.get("name")]
    selected_suite = f"test-affected ({', '.join(crates)})" if crates else "test-affected (none)"

    phases: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []
    total_passed = 0
    total_failed = 0
    total_not_run = 0

    for gate in data.get("gates", []):
        gate_name = str(gate.get("gate", "unknown"))
        kind = str(gate.get("kind", "execution"))
        phase_name = _classify_gate_phase(gate_name, kind)
        status = _normalize_status(str(gate.get("status", "NOT_RUN")))
        duration_ms = int(gate.get("duration_ms") or 0)

        phases.append(
            _make_phase(
                name=phase_name,
                stage=gate_name,
                duration_ms=duration_ms,
                status=status,
                cpu_user_ms=gate.get("cpu_user_ms", UNKNOWN),
                cpu_sys_ms=gate.get("cpu_sys_ms", UNKNOWN),
                cpu_pct=gate.get("cpu_pct", UNKNOWN),
                max_rss_kib=gate.get("max_rss_kib", UNKNOWN),
            )
        )

        log_passed = log_failed = log_ignored = 0
        log_failures: list[str] = []
        if logs_dir is not None:
            safe_gate = gate_name.replace(":", "_")
            log_file = logs_dir / f"{safe_gate}.log"
            log_passed, log_failed, log_ignored, log_failures = _parse_cargo_log_counts(log_file)

        if log_passed + log_failed + log_ignored > 0:
            total_passed += log_passed
            total_failed += log_failed
            total_not_run += log_ignored
        elif status == "PASSED":
            total_passed += 1
        elif status == "FAILED":
            total_failed += 1
        else:
            total_not_run += 1

        if status == "FAILED":
            summary = "; ".join(log_failures) if log_failures else str(gate.get("note", "gate failed"))
            failures.append(
                {
                    "stage": redact_text(gate_name),
                    "phase": phase_name,
                    "exit_code": gate.get("exit_code"),
                    "summary": redact_text(summary),
                }
            )

    ts = recorded_at or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    run_id = f"{ts.replace(':', '').replace('-', '')}-{sha[:7]}"
    resources = initial_resources or collect_initial_resources(limits=str(data.get("limits") or UNKNOWN))

    return sanitize_value(
        {
            "schema": SCHEMA,
            "run_id": run_id,
            "recorded_at": ts,
            "mode": str(data.get("mode") or "run"),
            "commit": {
                "sha": sha,
                "short": sha[:7],
                "branch": str(repo.get("branch") or "HEAD"),
                "base_commit": str(base.get("commit") or ""),
            },
            "selected_suite": selected_suite,
            "phases": phases,
            "total_duration_ms": int(data.get("duration_ms") or sum(p["duration_ms"] for p in phases)),
            "test_counts": {
                "PASSED": total_passed,
                "FAILED": total_failed,
                "NOT_RUN": total_not_run,
            },
            "initial_resources": resources,
            "failures": failures,
        }
    )


def build_run_from_perf_lab(
    report_path: Path,
    recorded_at: str | None = None,
) -> dict[str, Any]:
    """Convert a `perf-lab-*.json` report into `hodei.test-cost-history/v1`.

    Seeding time is recorded under `setup` and `seeding`, and NEVER counted as throughput.
    """
    data = json.loads(report_path.read_text(encoding="utf-8"))
    meta = data.get("meta", {})
    host_state = data.get("host_state", {})
    sha = str(meta.get("commit") or "0000000")
    profile = str(meta.get("profile") or "default")
    started_at = recorded_at or str(meta.get("started_at") or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))

    seed_ms = 0
    assets_total = 0
    evidence_total = 0
    for replica in data.get("replicas", []):
        seeds = replica.get("seeds", {})
        if seeds.get("mode") == "reused":
            seed_ms += int(round(float(seeds.get("mount_seconds") or 0.0) * 1000))
        else:
            seed_ms += int(round(float(seeds.get("catalog_bulk_seconds") or 0.0) * 1000))
            seed_ms += int(round(float(seeds.get("evidence_seconds") or 0.0) * 1000))
            assets_total += int(seeds.get("catalog_assets") or 0)
            evidence_total += int(seeds.get("evidence") or 0)

    agg = data.get("aggregate", {})
    workload_duration_ms = 0
    workload_throughput: dict[str, Any] = {}
    max_rss: Any = UNKNOWN
    for workload_name, stats in sorted(agg.items()):
        rps = stats.get("throughput_rps_median")
        if rps is not None:
            workload_throughput[workload_name] = float(rps)
        if "duration_ms" in stats and stats["duration_ms"] is not None:
            workload_duration_ms += int(stats["duration_ms"])
        if "peak_rss_kib_max" in stats and stats["peak_rss_kib_max"] is not None:
            current_rss = int(stats["peak_rss_kib_max"])
            max_rss = current_rss if max_rss == UNKNOWN else max(int(max_rss), current_rss)

    replica_failures = data.get("replica_failures", [])
    skips = data.get("skips_declared", [])
    replicas_completed = int(meta.get("replicas_completed") or 0)
    replicas_requested = int(meta.get("replicas_requested") or 0)

    phases = [
        _make_phase(
            name="setup",
            stage="seeding",
            duration_ms=seed_ms,
            status="PASSED" if replicas_completed > 0 else "NOT_RUN",
        ),
        _make_phase(
            name="execution",
            stage=f"perf-lab:{profile}",
            duration_ms=workload_duration_ms,
            status="FAILED" if replica_failures else ("PASSED" if replicas_completed > 0 else "NOT_RUN"),
            max_rss_kib=max_rss,
        ),
    ]

    failures = [
        {
            "stage": f"perf-lab:{profile}",
            "phase": "execution",
            "exit_code": None,
            "summary": redact_text(json.dumps(f, ensure_ascii=False)),
        }
        for f in replica_failures
    ]

    run_id = f"{started_at.replace(':', '').replace('-', '')}-{sha[:7]}"
    resources = collect_initial_resources(limits=f"perf-lab:{profile}", host_override=host_state)

    return sanitize_value(
        {
            "schema": SCHEMA,
            "run_id": run_id,
            "recorded_at": started_at,
            "mode": "lab",
            "commit": {
                "sha": sha,
                "short": sha[:7],
                "branch": "HEAD",
                "base_commit": "",
            },
            "selected_suite": f"perf-lab:{profile}",
            "phases": phases,
            "total_duration_ms": seed_ms + workload_duration_ms,
            "test_counts": {
                "PASSED": replicas_completed,
                "FAILED": len(replica_failures),
                "NOT_RUN": max(0, replicas_requested - replicas_completed) + len(skips),
            },
            "initial_resources": resources,
            "failures": failures,
            "seeding": {
                "duration_ms": seed_ms,
                "catalog_assets": assets_total,
                "evidence": evidence_total,
                "counted_as_throughput": False,
            },
            "throughput": {
                "workloads_rps": workload_throughput,
                "excludes_seeding": True,
            },
        }
    )


def build_run_from_pipeline_args(args: argparse.Namespace) -> dict[str, Any]:
    """Build a run record from explicit pipeline/stage CLI arguments."""
    git_info = _git_info_from_cwd()
    sha = args.commit or git_info["sha"]
    branch = args.branch or git_info["branch"]
    ts = args.recorded_at or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    status = _normalize_status(args.status)

    phase = _make_phase(
        name=args.phase,
        stage=args.stage,
        duration_ms=args.duration_ms,
        status=status,
        cpu_user_ms=args.cpu_user_ms,
        cpu_sys_ms=args.cpu_sys_ms,
        cpu_pct=args.cpu_pct,
        max_rss_kib=args.max_rss_kib,
    )

    failures: list[dict[str, Any]] = []
    if status == "FAILED":
        failures.append(
            {
                "stage": redact_text(args.stage),
                "phase": phase["name"],
                "exit_code": args.exit_code,
                "summary": redact_text(args.failure_summary or "stage failed"),
            }
        )

    passed = args.passed if args.passed is not None else (1 if status == "PASSED" else 0)
    failed = args.failed if args.failed is not None else (1 if status == "FAILED" else 0)
    not_run = args.not_run if args.not_run is not None else (1 if status == "NOT_RUN" else 0)

    run_id = f"{ts.replace(':', '').replace('-', '')}-{sha[:7]}"
    return sanitize_value(
        {
            "schema": SCHEMA,
            "run_id": run_id,
            "recorded_at": ts,
            "mode": "pipeline",
            "commit": {
                "sha": sha,
                "short": sha[:7],
                "branch": branch,
                "base_commit": "",
            },
            "selected_suite": args.suite or f"pipeline:{args.stage}",
            "phases": [phase],
            "total_duration_ms": phase["duration_ms"],
            "test_counts": {
                "PASSED": passed,
                "FAILED": failed,
                "NOT_RUN": not_run,
            },
            "initial_resources": collect_initial_resources(limits=args.limits),
            "failures": failures,
        }
    )


def validate_run(run: dict[str, Any]) -> list[str]:
    """Validate a run record against `hodei.test-cost-history/v1`. Returns error list."""
    errors: list[str] = []
    if run.get("schema") != SCHEMA:
        errors.append(f"invalid schema: expected {SCHEMA}, got {run.get('schema')!r}")

    for field in ("run_id", "recorded_at", "mode", "selected_suite"):
        if not isinstance(run.get(field), str) or not run[field]:
            errors.append(f"missing or invalid string field: {field}")

    commit = run.get("commit")
    if not isinstance(commit, dict) or not commit.get("sha"):
        errors.append("missing commit.sha")

    counts = run.get("test_counts")
    if not isinstance(counts, dict):
        errors.append("missing test_counts")
    else:
        for key in VALID_STATUSES:
            if not isinstance(counts.get(key), int) or counts[key] < 0:
                errors.append(f"invalid test_counts.{key}")

    phases = run.get("phases")
    if not isinstance(phases, list):
        errors.append("phases must be a list")
    else:
        for idx, phase in enumerate(phases):
            if phase.get("name") not in VALID_PHASES:
                errors.append(f"phases[{idx}].name invalid: {phase.get('name')!r}")
            if phase.get("status") not in VALID_STATUSES:
                errors.append(f"phases[{idx}].status invalid: {phase.get('status')!r}")
            for res_key in ("cpu_user_ms", "cpu_sys_ms", "cpu_pct", "max_rss_kib"):
                val = phase.get(res_key)
                if val != UNKNOWN and not isinstance(val, (int, float)):
                    errors.append(f"phases[{idx}].{res_key} must be number or UNKNOWN")

    resources = run.get("initial_resources")
    if not isinstance(resources, dict):
        errors.append("missing initial_resources")

    if not isinstance(run.get("failures"), list):
        errors.append("failures must be a list")

    serialized = json.dumps(run)
    for forbidden in FORBIDDEN_KEYS:
        if f'"{forbidden}"' in serialized.lower():
            errors.append(f"forbidden key serialized: {forbidden}")

    return errors


def _is_run_comparable(run: dict[str, Any]) -> bool:
    if run.get("mode") == "plan":
        return False
    counts = run.get("test_counts", {})
    executed = int(counts.get("PASSED", 0)) + int(counts.get("FAILED", 0))
    return executed > 0


def compute_trends(runs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Sort runs chronologically and attach comparable baseline comparisons without guessing."""
    ordered = sorted(runs, key=lambda r: (str(r.get("recorded_at", "")), str(r.get("run_id", ""))))
    enriched: list[dict[str, Any]] = []

    for idx, run in enumerate(ordered):
        item = sanitize_value(run)
        suite = item.get("selected_suite")
        mode = item.get("mode")
        baseline = None

        if _is_run_comparable(item):
            for prev in reversed(ordered[:idx]):
                if (
                    prev.get("selected_suite") == suite
                    and prev.get("mode") == mode
                    and _is_run_comparable(prev)
                ):
                    baseline = prev
                    break

        if baseline is None:
            item["baseline_comparison"] = {
                "status": "NO_COMPARABLE_BASELINE",
                "baseline_run_id": None,
                "baseline_commit": None,
                "delta_ms": None,
                "savings_pct": None,
            }
        else:
            cur_ms = int(item.get("total_duration_ms") or 0)
            base_ms = int(baseline.get("total_duration_ms") or 0)
            delta_ms = cur_ms - base_ms
            savings_pct = round(((base_ms - cur_ms) / base_ms) * 100.0, 2) if base_ms > 0 else None
            item["baseline_comparison"] = {
                "status": "COMPARABLE",
                "baseline_run_id": baseline.get("run_id"),
                "baseline_commit": baseline.get("commit", {}).get("short"),
                "delta_ms": delta_ms,
                "savings_pct": savings_pct,
            }

        enriched.append(item)

    return enriched


def render_html(runs: list[dict[str, Any]], out_html: Path) -> Path:
    """Render a 100% offline static HTML trend report from local `target/reports` runs."""
    enriched = compute_trends(runs)
    safe_json = json.dumps(enriched, indent=2, ensure_ascii=False).replace("</", "<\\/")

    total_runs = len(enriched)
    latest = enriched[-1] if enriched else None
    latest_commit = html.escape(str(latest["commit"]["short"])) if latest else "N/A"
    latest_suite = html.escape(str(latest["selected_suite"])) if latest else "N/A"
    latest_duration = f"{latest['total_duration_ms']} ms" if latest else "N/A"

    html_doc = f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Hodei Test Cost History (Offline)</title>
  <style>
    :root {{
      --bg: #0f172a;
      --surface: #1e293b;
      --border: #334155;
      --text: #e2e8f0;
      --muted: #94a3b8;
      --pass: #22c55e;
      --fail: #ef4444;
      --not-run: #f59e0b;
      --accent: #38bdf8;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      padding: 24px;
      font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif;
      background: var(--bg);
      color: var(--text);
      line-height: 1.5;
    }}
    h1, h2 {{ margin: 0 0 12px 0; }}
    .subtitle {{ color: var(--muted); margin-bottom: 20px; font-size: 0.92rem; }}
    .cards {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      gap: 12px;
      margin-bottom: 24px;
    }}
    .card {{
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 12px 14px;
    }}
    .card .label {{ color: var(--muted); font-size: 0.8rem; text-transform: uppercase; }}
    .card .value {{ font-size: 1.25rem; font-weight: 700; margin-top: 4px; }}
    .panel {{
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 16px;
      margin-bottom: 20px;
      overflow-x: auto;
    }}
    table {{ width: 100%; border-collapse: collapse; font-size: 0.88rem; }}
    th, td {{
      text-align: left;
      padding: 8px 10px;
      border-bottom: 1px solid var(--border);
      vertical-align: top;
    }}
    th {{ color: var(--muted); font-weight: 600; }}
    .badge {{
      display: inline-block;
      padding: 2px 7px;
      border-radius: 999px;
      font-size: 0.75rem;
      font-weight: 600;
    }}
    .badge-pass {{ background: rgba(34,197,94,0.18); color: var(--pass); }}
    .badge-fail {{ background: rgba(239,68,68,0.18); color: var(--fail); }}
    .badge-not-run {{ background: rgba(245,158,11,0.18); color: var(--not-run); }}
    .badge-unknown {{ background: rgba(148,163,184,0.18); color: var(--muted); }}
    code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.84em; }}
    .controls {{ display: flex; gap: 12px; margin-bottom: 12px; align-items: center; }}
    select {{
      background: var(--bg);
      color: var(--text);
      border: 1px solid var(--border);
      border-radius: 6px;
      padding: 6px 10px;
    }}
  </style>
</head>
<body>
  <h1>Hodei Test Cost History</h1>
  <div class="subtitle">
    Offline local report generated from <code>target/reports/runs/*.json</code> &middot; No external dependencies &middot; Redacted
  </div>

  <div class="cards">
    <div class="card"><div class="label">Recorded Runs</div><div class="value">{total_runs}</div></div>
    <div class="card"><div class="label">Latest Commit</div><div class="value"><code>{latest_commit}</code></div></div>
    <div class="card"><div class="label">Latest Suite</div><div class="value">{latest_suite}</div></div>
    <div class="card"><div class="label">Latest Total Time</div><div class="value">{latest_duration}</div></div>
  </div>

  <div class="panel">
    <h2>Duration Trend</h2>
    <svg id="trend-svg" width="100%" height="140" viewBox="0 0 800 140" preserveAspectRatio="none"></svg>
  </div>

  <div class="panel">
    <div class="controls">
      <h2 style="margin:0;">Runs</h2>
      <label for="suite-filter">Suite:</label>
      <select id="suite-filter"><option value="">All suites</option></select>
    </div>
    <table>
      <thead>
        <tr>
          <th>Recorded At</th>
          <th>Commit</th>
          <th>Suite</th>
          <th>Phases (ms)</th>
          <th>Total</th>
          <th>Tests (PASSED / FAILED / NOT_RUN)</th>
          <th>CPU / Max RSS</th>
          <th>Initial Resources</th>
          <th>Baseline Comparison</th>
        </tr>
      </thead>
      <tbody id="runs-body"></tbody>
    </table>
  </div>

  <div class="panel">
    <h2>Separated Failures</h2>
    <div id="failures-box"></div>
  </div>

  <script type="application/json" id="hodei-test-cost-history">{safe_json}</script>
  <script>
    (function () {{
      const raw = document.getElementById("hodei-test-cost-history").textContent || "[]";
      const runs = JSON.parse(raw);
      const suiteFilter = document.getElementById("suite-filter");
      const tbody = document.getElementById("runs-body");
      const failuresBox = document.getElementById("failures-box");
      const svg = document.getElementById("trend-svg");

      function esc(v) {{
        return String(v)
          .replace(/&/g, "&amp;")
          .replace(/</g, "&lt;")
          .replace(/>/g, "&gt;")
          .replace(/"/g, "&quot;");
      }}

      const suites = Array.from(new Set(runs.map(r => r.selected_suite))).sort();
      suites.forEach(s => {{
        const opt = document.createElement("option");
        opt.value = s;
        opt.textContent = s;
        suiteFilter.appendChild(opt);
      }});

      function renderTrend(filtered) {{
        svg.innerHTML = "";
        if (!filtered.length) return;
        const maxMs = Math.max(1, ...filtered.map(r => r.total_duration_ms || 0));
        const w = 800;
        const h = 140;
        const pad = 16;
        const step = filtered.length > 1 ? (w - pad * 2) / (filtered.length - 1) : 0;
        const pts = filtered.map((r, i) => {{
          const x = filtered.length > 1 ? pad + i * step : w / 2;
          const y = h - pad - ((r.total_duration_ms || 0) / maxMs) * (h - pad * 2);
          return `${{x.toFixed(1)}},${{y.toFixed(1)}}`;
        }}).join(" ");
        if (filtered.length > 1) {{
          svg.innerHTML += `<polyline fill="none" stroke="#38bdf8" stroke-width="2.5" points="${{pts}}" />`;
        }}
        filtered.forEach((r, i) => {{
          const x = filtered.length > 1 ? pad + i * step : w / 2;
          const y = h - pad - ((r.total_duration_ms || 0) / maxMs) * (h - pad * 2);
          svg.innerHTML += `<circle cx="${{x.toFixed(1)}}" cy="${{y.toFixed(1)}}" r="4" fill="#38bdf8"><title>${{esc(r.commit.short)}}: ${{r.total_duration_ms}} ms</title></circle>`;
        }});
      }}

      function renderTable(filtered) {{
        tbody.innerHTML = "";
        failuresBox.innerHTML = "";
        const allFailures = [];

        filtered.forEach(r => {{
          const phasesHtml = (r.phases || []).map(p =>
            `<div><code>${{esc(p.name)}}</code> (${{esc(p.stage)}}): ${{p.duration_ms}} ms [${{esc(p.status)}}]</div>`
          ).join("");

          const cpuSet = (r.phases || []).map(p => p.cpu_user_ms).filter(v => v !== "UNKNOWN");
          const rssSet = (r.phases || []).map(p => p.max_rss_kib).filter(v => v !== "UNKNOWN");
          const cpuLabel = cpuSet.length ? `${{cpuSet.reduce((a, b) => a + b, 0)}} ms` : "UNKNOWN";
          const rssLabel = rssSet.length ? `${{Math.max(...rssSet)}} KiB` : "UNKNOWN";

          const tc = r.test_counts || {{}};
          const ir = r.initial_resources || {{}};
          const bc = r.baseline_comparison || {{ status: "NO_COMPARABLE_BASELINE" }};
          const bcHtml = bc.status === "COMPARABLE"
            ? `${{bc.delta_ms >= 0 ? "+" : ""}}${{bc.delta_ms}} ms (${{bc.savings_pct !== null ? bc.savings_pct + "% saved" : "N/A"}} vs <code>${{esc(bc.baseline_commit)}}</code>)`
            : `<span class="badge badge-unknown">NO_COMPARABLE_BASELINE</span>`;

          const tr = document.createElement("tr");
          tr.innerHTML = `
            <td><code>${{esc(r.recorded_at)}}</code></td>
            <td><code>${{esc(r.commit.short)}}</code><br><small>${{esc(r.commit.branch)}}</small></td>
            <td>${{esc(r.selected_suite)}}</td>
            <td>${{phasesHtml}}</td>
            <td><strong>${{r.total_duration_ms}} ms</strong></td>
            <td>
              <span class="badge badge-pass">PASSED: ${{tc.PASSED ?? 0}}</span>
              <span class="badge badge-fail">FAILED: ${{tc.FAILED ?? 0}}</span>
              <span class="badge badge-not-run">NOT_RUN: ${{tc.NOT_RUN ?? 0}}</span>
            </td>
            <td>CPU: <code>${{esc(cpuLabel)}}</code><br>RSS: <code>${{esc(rssLabel)}}</code></td>
            <td>cores: <code>${{esc(ir.cpu_cores ?? "UNKNOWN")}}</code><br>mem: <code>${{esc(ir.mem_total_kib ?? "UNKNOWN")}} KiB</code><br>gov: <code>${{esc(ir.cpu_governor ?? "UNKNOWN")}}</code></td>
            <td>${{bcHtml}}</td>
          `;
          tbody.appendChild(tr);

          (r.failures || []).forEach(f => {{
            allFailures.push({{ commit: r.commit.short, suite: r.selected_suite, ...f }});
          }});
        }});

        if (!allFailures.length) {{
          failuresBox.innerHTML = "<p>No failures recorded.</p>";
        }} else {{
          const ul = document.createElement("ul");
          allFailures.forEach(f => {{
            const li = document.createElement("li");
            li.innerHTML = `<code>${{esc(f.commit)}}</code> [${{esc(f.stage)}} / ${{esc(f.phase)}}]: ${{esc(f.summary)}}`;
            ul.appendChild(li);
          }});
          failuresBox.appendChild(ul);
        }}
      }}

      function update() {{
        const selected = suiteFilter.value;
        const filtered = selected ? runs.filter(r => r.selected_suite === selected) : runs;
        renderTrend(filtered);
        renderTable(filtered);
      }}

      suiteFilter.addEventListener("change", update);
      update();
    }})();
  </script>
</body>
</html>
"""
    out_html.parent.mkdir(parents=True, exist_ok=True)
    out_html.write_text(html_doc, encoding="utf-8")
    return out_html


def load_runs_from_dir(runs_dir: Path) -> list[dict[str, Any]]:
    if not runs_dir.is_dir():
        return []
    loaded: list[dict[str, Any]] = []
    for path in sorted(runs_dir.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        loaded.append(sanitize_value(data))
    return loaded


def cmd_record(args: argparse.Namespace) -> int:
    out_dir = Path(args.out_dir)
    runs_dir = out_dir / "runs"
    runs_dir.mkdir(parents=True, exist_ok=True)

    if args.source_type == "test-affected":
        logs_dir = Path(args.logs_dir) if args.logs_dir else None
        run = build_run_from_test_affected(Path(args.input), logs_dir=logs_dir, recorded_at=args.recorded_at)
    elif args.source_type == "perf-lab":
        run = build_run_from_perf_lab(Path(args.input), recorded_at=args.recorded_at)
    else:
        run = build_run_from_pipeline_args(args)

    errors = validate_run(run)
    if errors:
        print(f"validation errors: {errors}", file=sys.stderr)
        return 1

    out_file = runs_dir / f"{run['run_id']}.json"
    out_file.write_text(json.dumps(run, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"recorded: {out_file}")

    if args.render:
        runs = load_runs_from_dir(runs_dir)
        html_path = render_html(runs, out_dir / "index.html")
        print(f"rendered: {html_path}")

    return 0


def cmd_render(args: argparse.Namespace) -> int:
    out_dir = Path(args.out_dir)
    runs_dir = Path(args.runs_dir) if args.runs_dir else (out_dir / "runs")
    runs = load_runs_from_dir(runs_dir)
    html_path = render_html(runs, out_dir / "index.html")
    print(f"rendered: {html_path} ({len(runs)} runs)")
    return 0


def cmd_validate(args: argparse.Namespace) -> int:
    failed = False
    for path_str in args.files:
        data = json.loads(Path(path_str).read_text(encoding="utf-8"))
        errors = validate_run(data)
        if errors:
            print(f"INVALID {path_str}: {errors}", file=sys.stderr)
            failed = True
        else:
            print(f"VALID {path_str}")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    rec = subparsers.add_parser("record", help="record a run JSON into target/reports/runs/")
    rec.add_argument("--source-type", choices=("test-affected", "perf-lab", "pipeline"), required=True)
    rec.add_argument("--input", default="", help="path to source JSON report")
    rec.add_argument("--logs-dir", default="", help="optional logs dir for cargo test counts")
    rec.add_argument("--out-dir", default="target/reports", help="reports output directory")
    rec.add_argument("--recorded-at", default=None, help="override ISO-8601 UTC timestamp")
    rec.add_argument("--render", action="store_true", help="also render index.html")
    rec.add_argument("--stage", default="pipeline")
    rec.add_argument("--phase", default="execution", choices=VALID_PHASES)
    rec.add_argument("--status", default="PASSED")
    rec.add_argument("--suite", default="")
    rec.add_argument("--commit", default="")
    rec.add_argument("--branch", default="")
    rec.add_argument("--duration-ms", type=int, default=0)
    rec.add_argument("--passed", type=int, default=None)
    rec.add_argument("--failed", type=int, default=None)
    rec.add_argument("--not-run", type=int, default=None)
    rec.add_argument("--cpu-user-ms", default=UNKNOWN)
    rec.add_argument("--cpu-sys-ms", default=UNKNOWN)
    rec.add_argument("--cpu-pct", default=UNKNOWN)
    rec.add_argument("--max-rss-kib", default=UNKNOWN)
    rec.add_argument("--limits", default=UNKNOWN)
    rec.add_argument("--exit-code", type=int, default=None)
    rec.add_argument("--failure-summary", default="")
    rec.set_defaults(handler=cmd_record)

    ren = subparsers.add_parser("render", help="render static offline HTML trend report")
    ren.add_argument("--out-dir", default="target/reports")
    ren.add_argument("--runs-dir", default="")
    ren.set_defaults(handler=cmd_render)

    val = subparsers.add_parser("validate", help="validate run JSON files")
    val.add_argument("files", nargs="+")
    val.set_defaults(handler=cmd_validate)

    args = parser.parse_args()
    return args.handler(args)


if __name__ == "__main__":
    raise SystemExit(main())
