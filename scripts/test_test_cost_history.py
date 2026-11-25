#!/usr/bin/env python3
"""Unit and fixture tests for `scripts/test-cost-history.py` (HSP-245)."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

SCRIPT_PATH = Path(__file__).resolve().parent / "test-cost-history.py"


def load_module():
    spec = importlib.util.spec_from_file_location("test_cost_history", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


tch = load_module()


class TestCostHistoryTests(unittest.TestCase):
    def test_two_commit_fixture_and_html_reproducibility(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports_dir = root / "target" / "reports"
            inputs_dir = reports_dir / "inputs"
            logs_dir = inputs_dir / "logs"
            logs_dir.mkdir(parents=True)

            (logs_dir / "test_hodei-api.log").write_text(
                "test result: ok. 14 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out\n",
                encoding="utf-8",
            )
            (logs_dir / "test_hodei-core.log").write_text(
                "test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n"
                "thread 'authz::rejects_empty' panicked at src/lib.rs:42\n",
                encoding="utf-8",
            )

            report_1 = inputs_dir / "report1.json"
            report_1.write_text(
                json.dumps(
                    {
                        "schema": "hodei.test-affected/v1",
                        "mode": "run",
                        "result": "PASS",
                        "duration_ms": 1500,
                        "limits": "bounded",
                        "repository": {
                            "root": "/home/alice/secret-workspace",
                            "commit": "1111111aaaaaaa",
                            "branch": "feat/one",
                            "base": {"commit": "0000000"},
                        },
                        "affected_crates": [{"name": "hodei-api"}],
                        "gates": [
                            {"gate": "fmt:hodei-api", "kind": "fmt", "status": "PASS", "duration_ms": 200, "exit_code": 0},
                            {"gate": "clippy:hodei-api", "kind": "clippy", "status": "PASS", "duration_ms": 500, "exit_code": 0},
                            {"gate": "test:hodei-api", "kind": "test", "status": "PASS", "duration_ms": 800, "exit_code": 0},
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report_2 = inputs_dir / "report2.json"
            report_2.write_text(
                json.dumps(
                    {
                        "schema": "hodei.test-affected/v1",
                        "mode": "run",
                        "result": "FAIL",
                        "duration_ms": 1200,
                        "limits": "none",
                        "repository": {
                            "root": "/home/alice/secret-workspace",
                            "commit": "2222222bbbbbbb",
                            "branch": "feat/two",
                            "base": {"commit": "1111111"},
                        },
                        "affected_crates": [{"name": "hodei-api"}],
                        "gates": [
                            {"gate": "fmt:hodei-api", "kind": "fmt", "status": "PASS", "duration_ms": 150, "exit_code": 0},
                            {"gate": "clippy:hodei-api", "kind": "clippy", "status": "PASS", "duration_ms": 450, "exit_code": 0},
                            {
                                "gate": "test:hodei-core",
                                "kind": "test",
                                "status": "FAIL",
                                "duration_ms": 600,
                                "exit_code": 101,
                                "note": "failed on /home/alice/repo",
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )

            for rpt, ts in ((report_1, "2026-11-24T10:00:00Z"), (report_2, "2026-11-24T10:05:00Z")):
                proc = subprocess.run(
                    [
                        sys.executable,
                        str(SCRIPT_PATH),
                        "record",
                        "--source-type",
                        "test-affected",
                        "--input",
                        str(rpt),
                        "--logs-dir",
                        str(logs_dir),
                        "--out-dir",
                        str(reports_dir),
                        "--recorded-at",
                        ts,
                        "--render",
                    ],
                    capture_output=True,
                    text=True,
                    check=True,
                )
                self.assertIn("recorded:", proc.stdout)

            run_files = sorted((reports_dir / "runs").glob("*.json"))
            self.assertEqual(len(run_files), 2)
            for rf in run_files:
                self.assertEqual(tch.validate_run(json.loads(rf.read_text(encoding="utf-8"))), [])

            run1 = json.loads(run_files[0].read_text(encoding="utf-8"))
            run2 = json.loads(run_files[1].read_text(encoding="utf-8"))

            self.assertEqual(run1["commit"]["short"], "1111111")
            self.assertEqual(run2["commit"]["short"], "2222222")
            self.assertEqual(run1["selected_suite"], "test-affected (hodei-api)")
            # Tests and gates are counted apart and never mixed in one counter.
            self.assertEqual(run1["counts_unit"], "tests")
            self.assertEqual(run1["test_counts"], {"PASSED": 14, "FAILED": 0, "NOT_RUN": 2})
            self.assertEqual(run1["gate_counts"], {"PASSED": 3, "FAILED": 0, "NOT_RUN": 0})
            self.assertEqual(run2["test_counts"], {"PASSED": 9, "FAILED": 1, "NOT_RUN": 0})
            self.assertEqual(run2["gate_counts"], {"PASSED": 2, "FAILED": 1, "NOT_RUN": 0})
            self.assertEqual(len(run2["failures"]), 1)
            self.assertEqual(run2["failures"][0]["stage"], "test:hodei-core")
            self.assertIn("initial_resources", run1)

            html_first = (reports_dir / "index.html").read_text(encoding="utf-8")
            subprocess.run(
                [sys.executable, str(SCRIPT_PATH), "render", "--out-dir", str(reports_dir)],
                check=True,
                capture_output=True,
                text=True,
            )
            html_second = (reports_dir / "index.html").read_text(encoding="utf-8")
            self.assertEqual(html_first, html_second)

            self.assertIn("1111111", html_first)
            self.assertIn("2222222", html_first)
            self.assertIn("test-affected (hodei-api)", html_first)
            self.assertIn("PASSED", html_first)
            self.assertIn("FAILED", html_first)
            self.assertIn("NOT_RUN", html_first)
            self.assertIn("NO_COMPARABLE_BASELINE", html_first)
            # bounded -> none: the same suite is NOT a baseline for the second run.
            self.assertEqual(run1["initial_resources"]["limits"], "bounded")
            self.assertEqual(run2["initial_resources"]["limits"], "none")
            self.assertEqual(tch.compute_trends([run1, run2])[1]["baseline_comparison"]["reason"], "LIMITS_MISMATCH")

    def test_unknown_cpu_rss_and_no_inferred_savings_without_comparable_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rpt_a = root / "a.json"
            rpt_b = root / "b.json"

            rpt_a.write_text(
                json.dumps(
                    {
                        "schema": "hodei.test-affected/v1",
                        "mode": "run",
                        "duration_ms": 1000,
                        "repository": {"commit": "aaaa111", "branch": "main", "base": {"commit": "0000"}},
                        "affected_crates": [{"name": "crate-a"}],
                        "gates": [{"gate": "test:crate-a", "kind": "test", "status": "PASS", "duration_ms": 1000, "exit_code": 0}],
                    }
                ),
                encoding="utf-8",
            )
            rpt_b.write_text(
                json.dumps(
                    {
                        "schema": "hodei.test-affected/v1",
                        "mode": "run",
                        "duration_ms": 500,
                        "repository": {"commit": "bbbb222", "branch": "feat", "base": {"commit": "aaaa111"}},
                        "affected_crates": [{"name": "crate-b"}],
                        "gates": [{"gate": "test:crate-b", "kind": "test", "status": "PASS", "duration_ms": 500, "exit_code": 0}],
                    }
                ),
                encoding="utf-8",
            )

            run_a = tch.build_run_from_test_affected(
                rpt_a,
                recorded_at="2026-11-24T11:00:00Z",
                initial_resources=tch.collect_initial_resources(host_override={}),
            )
            run_b = tch.build_run_from_test_affected(
                rpt_b,
                recorded_at="2026-11-24T11:05:00Z",
                initial_resources=tch.collect_initial_resources(host_override={}),
            )

            for key in ("cpu_user_ms", "cpu_sys_ms", "cpu_pct", "max_rss_kib"):
                self.assertEqual(run_a["phases"][0][key], "UNKNOWN")
            self.assertEqual(run_a["initial_resources"]["cpu_cores"], "UNKNOWN")
            self.assertEqual(run_a["initial_resources"]["mem_total_kib"], "UNKNOWN")

            trends = tch.compute_trends([run_a, run_b])
            self.assertEqual(trends[0]["baseline_comparison"]["status"], "NO_COMPARABLE_BASELINE")
            self.assertIsNone(trends[0]["baseline_comparison"]["savings_pct"])
            self.assertEqual(trends[1]["baseline_comparison"]["status"], "NO_COMPARABLE_BASELINE")
            self.assertIsNone(trends[1]["baseline_comparison"]["savings_pct"])

    def test_perf_lab_seeding_never_counted_as_throughput(self):
        with tempfile.TemporaryDirectory() as tmp:
            perf_path = Path(tmp) / "perf-lab-smoke.json"
            perf_path.write_text(
                json.dumps(
                    {
                        "meta": {
                            "profile": "smoke",
                            "commit": "cccc333",
                            "started_at": "2026-11-24T12:00:00Z",
                            "replicas_requested": 2,
                            "replicas_completed": 1,
                        },
                        "host_state": {"nproc": None, "mem_total": None},
                        "replicas": [
                            {
                                "replica": 1,
                                "seeds": {"mode": "fresh", "catalog_assets": 1000, "catalog_bulk_seconds": 12.5, "evidence": 500, "evidence_seconds": 2.5},
                            }
                        ],
                        "aggregate": {
                            "lookup": {"throughput_rps_median": 420.5, "duration_ms": 3000}
                        },
                        "replica_failures": [{"replica": 2, "error": "timeout"}],
                        "skips_declared": [{"id": "skip-1", "motive": "declared"}],
                    }
                ),
                encoding="utf-8",
            )

            run = tch.build_run_from_perf_lab(perf_path)
            self.assertEqual(run["seeding"]["duration_ms"], 15000)
            self.assertFalse(run["seeding"]["counted_as_throughput"])
            self.assertTrue(run["throughput"]["excludes_seeding"])
            self.assertEqual(run["throughput"]["workloads_rps"], {"lookup": 420.5})
            self.assertEqual(run["initial_resources"]["cpu_cores"], "UNKNOWN")
            self.assertEqual(run["initial_resources"]["mem_total_kib"], "UNKNOWN")
            self.assertEqual(run["test_counts"], {"PASSED": 1, "FAILED": 1, "NOT_RUN": 2})
            self.assertEqual(run["counts_unit"], "replicas")
            # Seeding is out of the comparable total: only the workload counts.
            self.assertEqual(run["total_duration_ms"], 3000)
            self.assertEqual(tch.validate_run(run), [])

    def test_offline_html_and_pii_secret_redaction(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports_dir = root / "target" / "reports"
            runs_dir = reports_dir / "runs"
            runs_dir.mkdir(parents=True)

            leaked_report = root / "leaked.json"
            leaked_report.write_text(
                json.dumps(
                    {
                        "schema": "hodei.test-affected/v1",
                        "mode": "run",
                        "duration_ms": 250,
                        "limits": "bounded",
                        "root": "/home/rubentxu/private",
                        "user": "rubentxu",
                        "hostname": "dev-laptop.local",
                        "repository": {
                            "root": "/home/rubentxu/private",
                            "commit": "dddd444",
                            "branch": "feat/sec",
                            "base": {"commit": "cccc333"},
                        },
                        "affected_crates": [{"name": "hodei-auth"}],
                        "gates": [
                            {
                                "gate": "test:hodei-auth",
                                "kind": "test",
                                "status": "FAIL",
                                "duration_ms": 250,
                                "exit_code": 1,
                                "note": (
                                    "contact alice@example.com at /home/alice/work "
                                    "with Bearer eyJhbGciOiJIUzI1NiJ9.secret "
                                    "ghp_1234567890abcdefghij123456 "
                                    "AWS_SECRET_ACCESS_KEY=topsecret123"
                                ),
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            run = tch.build_run_from_test_affected(leaked_report, recorded_at="2026-11-24T12:10:00Z")
            self.assertEqual(tch.validate_run(run), [])
            run_file = runs_dir / f"{run['run_id']}.json"
            run_file.write_text(json.dumps(run, indent=2) + "\n", encoding="utf-8")

            html_path = tch.render_html([run], reports_dir / "index.html")
            html_text = html_path.read_text(encoding="utf-8")
            run_text = run_file.read_text(encoding="utf-8")

            for secret in (
                "alice@example.com",
                "/home/alice",
                "/home/rubentxu",
                "eyJhbGciOiJIUzI1NiJ9.secret",
                "ghp_1234567890abcdefghij123456",
                "topsecret123",
                "dev-laptop.local",
            ):
                self.assertNotIn(secret, run_text)
                self.assertNotIn(secret, html_text)

            self.assertIsNone(re.search(r"https?://", html_text))


def _run(duration_ms, limits="bounded", cores=8, mem=16000000, suite="suite-x", unit="tests", at="2026-11-24T10:00:00Z"):
    """A minimal valid run with chosen limits and resources."""
    phase = tch._make_phase("execution", "gate", duration_ms, "PASSED")
    return {
        "schema": tch.SCHEMA,
        "run_id": at + suite + str(duration_ms),
        "recorded_at": at,
        "mode": "run",
        "commit": {"sha": "abc1234", "short": "abc1234", "branch": "b", "base_commit": ""},
        "selected_suite": suite,
        "phases": [phase],
        "total_duration_ms": duration_ms,
        "counts_unit": unit,
        "test_counts": {"PASSED": 3, "FAILED": 0, "NOT_RUN": 0},
        "initial_resources": {"cpu_cores": cores, "mem_total_kib": mem, "limits": limits},
        "failures": [],
    }


class ComparabilityTests(unittest.TestCase):
    def test_same_limits_and_resources_are_comparable_with_numeric_delta(self):
        old = _run(1000, at="2026-11-24T10:00:00Z")
        new = _run(750, at="2026-11-24T10:05:00Z")
        comparison = tch.compute_trends([old, new])[1]["baseline_comparison"]
        self.assertEqual(comparison["status"], "COMPARABLE")
        self.assertEqual(comparison["delta_ms"], -250)
        self.assertEqual(comparison["savings_pct"], 25.0)

    def test_bounded_against_none_is_not_comparable(self):
        old = _run(1000, limits="bounded", at="2026-11-24T10:00:00Z")
        new = _run(400, limits="none (--no-bounded)", at="2026-11-24T10:05:00Z")
        comparison = tch.compute_trends([old, new])[1]["baseline_comparison"]
        self.assertEqual(comparison["status"], "NO_COMPARABLE_BASELINE")
        self.assertEqual(comparison["reason"], "LIMITS_MISMATCH")
        self.assertIsNone(comparison["savings_pct"])

    def test_different_resources_or_unknown_are_not_comparable(self):
        old = _run(1000, cores=8, at="2026-11-24T10:00:00Z")
        for other in (_run(500, cores=4, at="2026-11-24T10:05:00Z"), _run(500, mem="UNKNOWN", at="2026-11-24T10:05:00Z")):
            comparison = tch.compute_trends([old, other])[1]["baseline_comparison"]
            self.assertEqual(comparison["status"], "NO_COMPARABLE_BASELINE")
            self.assertEqual(comparison["reason"], "RESOURCES_MISMATCH")

    def test_different_counts_unit_is_not_comparable(self):
        old = _run(1000, unit="tests", at="2026-11-24T10:00:00Z")
        new = _run(500, unit="stages", at="2026-11-24T10:05:00Z")
        comparison = tch.compute_trends([old, new])[1]["baseline_comparison"]
        self.assertEqual(comparison["reason"], "COUNTS_UNIT_MISMATCH")


class PerfLabSeedingTests(unittest.TestCase):
    def test_reused_seed_mount_is_outside_the_comparable_total(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "perf-lab-lab.json"
            path.write_text(
                json.dumps(
                    {
                        "meta": {"profile": "lab", "commit": "eeee555", "started_at": "2026-11-24T13:00:00Z",
                                 "replicas_requested": 1, "replicas_completed": 1},
                        "host_state": {},
                        "replicas": [{"replica": 1, "seeds": {"mode": "reused", "mount_seconds": 4.0}}],
                        "aggregate": {"lookup": {"throughput_rps_median": 100.0, "duration_ms": 2000}},
                        "replica_failures": [],
                        "skips_declared": [],
                    }
                ),
                encoding="utf-8",
            )
            run = tch.build_run_from_perf_lab(path)
            self.assertEqual(run["seeding"]["mode"], "reused")
            self.assertEqual(run["seeding"]["duration_ms"], 4000)
            self.assertFalse(run["seeding"]["counted_in_total"])
            self.assertEqual(run["total_duration_ms"], 2000)
            seeding_phase = next(p for p in run["phases"] if p["stage"] == "seeding")
            self.assertFalse(seeding_phase["counted_in_total"])
            self.assertEqual(tch.validate_run(run), [])

    def test_a_total_that_includes_seeding_is_invalid(self):
        run = _run(1000)
        run["phases"].append(tch._make_phase("setup", "seeding", 5000, "PASSED", counted_in_total=False))
        run["total_duration_ms"] = 6000
        self.assertTrue(any("total_duration_ms" in e for e in tch.validate_run(run)))


class ConfinementTests(unittest.TestCase):
    def _record(self, args, cwd):
        return subprocess.run([sys.executable, str(SCRIPT_PATH), *args], capture_output=True, text=True, cwd=cwd)

    def test_input_outside_target_reports_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            reports.mkdir(parents=True)
            outside = root / "elsewhere.json"
            outside.write_text("{}", encoding="utf-8")
            proc = self._record(
                ["record", "--source-type", "test-affected", "--input", str(outside), "--out-dir", str(reports)], root
            )
            self.assertEqual(proc.returncode, 2)
            self.assertIn("must be inside target/reports", proc.stderr)
            self.assertEqual(list((reports / "runs").glob("*.json")) if (reports / "runs").exists() else [], [])

    def test_runs_dir_outside_and_symlink_escape_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            reports.mkdir(parents=True)
            outside = root / "other-runs"
            outside.mkdir()
            proc = self._record(["render", "--out-dir", str(reports), "--runs-dir", str(outside)], root)
            self.assertEqual(proc.returncode, 2)
            self.assertIn("--runs-dir must be inside target/reports", proc.stderr)

            (reports / "runs").mkdir()
            (outside / "x.json").write_text(json.dumps(_run(10)), encoding="utf-8")
            (reports / "runs" / "link.json").symlink_to(outside / "x.json")
            proc = self._record(["render", "--out-dir", str(reports)], root)
            self.assertEqual(proc.returncode, 2)
            self.assertIn("run file must be inside target/reports", proc.stderr)

    def test_out_dir_must_be_a_target_reports_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._record(["render", "--out-dir", tmp], Path(tmp))
            self.assertEqual(proc.returncode, 2)
            self.assertIn("target/reports", proc.stderr)

    def test_input_inside_target_reports_is_accepted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            (reports / "inputs").mkdir(parents=True)
            report = reports / "inputs" / "r.json"
            report.write_text(
                json.dumps({"mode": "run", "repository": {"commit": "f" * 7}, "gates": []}), encoding="utf-8"
            )
            proc = self._record(
                ["record", "--source-type", "test-affected", "--input", str(report), "--out-dir", str(reports)], root
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)


class PipelineStageTests(unittest.TestCase):
    def _cli(self, args, root):
        return subprocess.run(
            [sys.executable, str(SCRIPT_PATH), *args], capture_output=True, text=True, cwd=root
        )

    def test_duration_is_wall_time_since_the_start_stamp_and_status_is_real(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            reports.mkdir(parents=True)
            self.assertEqual(self._cli(["pipeline-start", "--out-dir", str(reports), "--now-ms", "1000000"], root).returncode, 0)
            proc = self._cli(
                ["record", "--source-type", "pipeline", "--status", "auto", "--out-dir", str(reports),
                 "--started-at-file", str(reports / ".pipeline-start.json"), "--now-ms", "1042500",
                 "--recorded-at", "2026-11-24T14:00:00Z"],
                root,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            run = json.loads(next((reports / "runs").glob("*.json")).read_text(encoding="utf-8"))
            self.assertEqual(run["total_duration_ms"], 42500)
            self.assertTrue(run["phases"][0]["duration_measured"])
            self.assertEqual(run["phases"][0]["status"], "PASSED")
            self.assertEqual(run["counts_unit"], "stages")
            self.assertFalse((reports / ".pipeline-start.json").exists())

    def test_a_run_that_never_finished_is_recorded_as_failed_by_the_next_start(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            reports.mkdir(parents=True)
            self._cli(["pipeline-start", "--out-dir", str(reports), "--now-ms", "1000000"], root)
            # The first run died: no record. The second start finds the stale stamp.
            proc = self._cli(["pipeline-start", "--out-dir", str(reports), "--now-ms", "2000000"], root)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            runs = [json.loads(f.read_text(encoding="utf-8")) for f in (reports / "runs").glob("*.json")]
            self.assertEqual(len(runs), 1)
            self.assertEqual(runs[0]["phases"][0]["status"], "FAILED")
            self.assertEqual(runs[0]["test_counts"]["FAILED"], 1)
            self.assertEqual(len(runs[0]["failures"]), 1)
            self.assertFalse(runs[0]["phases"][0]["duration_measured"])
            # An unmeasured duration is never a baseline.
            self.assertEqual(tch.compute_trends(runs)[0]["baseline_comparison"]["reason"], "NOT_EXECUTED")

    def test_without_a_stamp_the_stage_is_not_reported_as_passed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            reports = root / "target" / "reports"
            reports.mkdir(parents=True)
            self._cli(["record", "--source-type", "pipeline", "--status", "auto", "--out-dir", str(reports)], root)
            run = json.loads(next((reports / "runs").glob("*.json")).read_text(encoding="utf-8"))
            self.assertEqual(run["phases"][0]["status"], "NOT_RUN")
            self.assertFalse(run["phases"][0]["duration_measured"])

    def test_pipeline_kts_reads_real_status_and_duration(self):
        text = (Path(__file__).resolve().parent.parent / "pipeline.kts").read_text(encoding="utf-8")
        report_stage = text[text.index('stage("report (test cost history)")'):]
        self.assertIn('stage("start (test cost history)")', text)
        self.assertIn("pipeline-start", text)
        self.assertIn("--started-at-file", report_stage)
        self.assertIn("--status auto", report_stage)
        self.assertNotIn("--status PASSED", report_stage)
        self.assertNotIn("--duration-ms", report_stage)


def _make_repo(root, recorder_ok=True, cargo_script=""):
    """A throwaway git repo with the real shell script and recorder, a fake cargo and a stub planner."""
    scripts = root / "scripts"
    scripts.mkdir()
    real_dir = Path(__file__).resolve().parent
    for name in ("test-affected.sh", "test-affected.py"):
        (scripts / name).write_text((real_dir / name).read_text(encoding="utf-8"), encoding="utf-8")
    (scripts / "real-planner.py").write_text((real_dir / "test-affected.py").read_text(encoding="utf-8"), encoding="utf-8")
    # The stub planner invents one package gate and delegates `report` to the real planner.
    (scripts / "test-affected.py").write_text(
        "import json, runpy, sys\n"
        "cmd = sys.argv[1]\n"
        "if cmd == 'plan':\n"
        "    a = sys.argv\n"
        "    out, tsv = a[a.index('--out') + 1], a[a.index('--tsv') + 1]\n"
        "    gate = {'gate': 'test:foo', 'kind': 'test', 'package': 'foo', 'command': 'cargo test -p foo', 'reason': 'r'}\n"
        "    json.dump({'schema': 's', 'repository': {'commit': 'c' * 40, 'branch': 'main', 'base': {'commit': 'b'}},\n"
        "               'causes': [], 'affected_crates': [{'name': 'foo'}], 'gates': [gate]}, open(out, 'w'))\n"
        "    open(tsv, 'w').write('test:foo\\ttest\\tfoo\\tcargo test -p foo\\tr\\n')\n"
        "elif cmd == 'explain':\n"
        "    pass\n"
        "else:\n"
        f"    sys.argv = ['x'] + sys.argv[1:]\n"
        f"    runpy.run_path(r'{scripts / 'real-planner.py'}', run_name='__main__')\n",
        encoding="utf-8",
    )
    if recorder_ok:
        (scripts / "test-cost-history.py").write_text((real_dir / "test-cost-history.py").read_text(encoding="utf-8"), encoding="utf-8")
    else:
        (scripts / "test-cost-history.py").write_text("import sys\nsys.exit(1)\n", encoding="utf-8")
    bin_dir = root / "bin"
    bin_dir.mkdir()
    cargo = bin_dir / "cargo"
    cargo.write_text("#!/usr/bin/env bash\n" + cargo_script, encoding="utf-8")
    cargo.chmod(0o755)
    # Plumbing with an empty global config: the throwaway repo only needs a base
    # commit, and the machine-wide commit policy must not judge it.
    genv = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull}
    git = ["git", "-C", str(root), "-c", "user.email=t@example.test", "-c", "user.name=t"]
    subprocess.run(["git", "init", "-q", "-b", "main", str(root)], check=True, env=genv)
    tree = subprocess.run([*git, "write-tree"], capture_output=True, text=True, check=True, env=genv).stdout.strip()
    commit = subprocess.run(
        [*git, "commit-tree", tree, "-m", "init"], capture_output=True, text=True, check=True, env=genv
    ).stdout.strip()
    subprocess.run([*git, "update-ref", "refs/heads/main", commit], check=True, env=genv)
    return bin_dir


def _run_script(root, bin_dir):
    env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}", "GIT_CONFIG_GLOBAL": os.devnull}
    return subprocess.run(
        ["bash", str(root / "scripts" / "test-affected.sh"), "--no-bounded", "--no-clippy", "main"],
        capture_output=True, text=True, cwd=root, env=env,
    )


FAKE_CARGO_OK = """\
case "$1 $*" in
  *--no-run*) sleep 0.2 ;;
  "check "*) sleep 0.1 ;;
  *) python3 -c "x = bytearray(40 * 1024 * 1024); import time; time.sleep(0.3)"
     echo 'test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out' ;;
esac
"""


class RealScriptTests(unittest.TestCase):
    def test_compile_link_and_execution_are_measured_apart_with_cpu_and_rss(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bin_dir = _make_repo(root, cargo_script=FAKE_CARGO_OK)
            proc = _run_script(root, bin_dir)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            report = json.loads((root / "target" / "test-affected" / "report.json").read_text(encoding="utf-8"))
            phases = {p["phase"]: p for p in report["gates"][0]["phases"]}
            self.assertEqual(set(phases), {"compile", "link", "execution"})
            self.assertGreaterEqual(phases["link"]["duration_ms"], 150)
            self.assertGreaterEqual(phases["execution"]["duration_ms"], 250)
            self.assertLess(phases["compile"]["duration_ms"], phases["link"]["duration_ms"] + 400)
            has_time = os.access("/usr/bin/time", os.X_OK)
            if has_time:
                self.assertIsInstance(phases["execution"]["max_rss_kib"], int)
                self.assertGreater(phases["execution"]["max_rss_kib"], 0)
                self.assertIsInstance(phases["execution"]["cpu_user_ms"], int)
            else:
                self.assertEqual(phases["execution"]["max_rss_kib"], "UNKNOWN")
            # The history run holds the same three phases and the real test counts.
            run = json.loads(next((root / "target" / "reports" / "runs").glob("*.json")).read_text(encoding="utf-8"))
            self.assertEqual([p["name"] for p in run["phases"]], ["compile", "link", "execution"])
            self.assertEqual(run["test_counts"], {"PASSED": 3, "FAILED": 0, "NOT_RUN": 1})
            self.assertEqual(run["gate_counts"]["PASSED"], 1)
            self.assertEqual(run["total_duration_ms"], sum(p["duration_ms"] for p in run["phases"]))

    def test_a_failing_compile_leaves_link_and_execution_not_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bin_dir = _make_repo(root, cargo_script='case "$1" in check) echo "error: boom"; exit 101 ;; esac\n')
            proc = _run_script(root, bin_dir)
            self.assertEqual(proc.returncode, 1)
            report = json.loads((root / "target" / "test-affected" / "report.json").read_text(encoding="utf-8"))
            statuses = {p["phase"]: p["status"] for p in report["gates"][0]["phases"]}
            self.assertEqual(statuses, {"compile": "FAIL", "link": "NOT_RUN", "execution": "NOT_RUN"})

    def test_a_failed_registration_warns_on_stderr_and_keeps_the_verdict(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bin_dir = _make_repo(root, recorder_ok=False, cargo_script=FAKE_CARGO_OK)
            proc = _run_script(root, bin_dir)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("aviso: no se pudo registrar el historico", proc.stderr)
            self.assertNotIn("aviso: no se pudo registrar", proc.stdout)

    def test_the_script_has_no_silent_swallow_of_the_registration(self):
        text = (Path(__file__).resolve().parent / "test-affected.sh").read_text(encoding="utf-8")
        tail = text[text.index("test-cost-history.py"):]
        self.assertNotIn("|| true", tail)


if __name__ == "__main__":
    unittest.main()
