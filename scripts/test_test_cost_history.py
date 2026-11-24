#!/usr/bin/env python3
"""Unit and fixture tests for `scripts/test-cost-history.py` (HSP-245)."""

from __future__ import annotations

import importlib.util
import json
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
            logs_dir = root / "logs"
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

            report_1 = root / "report1.json"
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

            report_2 = root / "report2.json"
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
            self.assertEqual(run1["test_counts"], {"PASSED": 16, "FAILED": 0, "NOT_RUN": 2})
            self.assertEqual(run2["test_counts"], {"PASSED": 11, "FAILED": 1, "NOT_RUN": 0})
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
            self.assertIn("COMPARABLE", html_first)
            self.assertIn("NO_COMPARABLE_BASELINE", html_first)

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


if __name__ == "__main__":
    unittest.main()
