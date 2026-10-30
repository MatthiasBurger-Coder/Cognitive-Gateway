import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("runner", ROOT / "tests/scripts/quality-gate.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.host = patch.object(runner._HOST, "CognitiveTestHost")
        host = self.host.start().return_value.__enter__.return_value
        host.environment = {}
        host.report = {"host": "mock-postgres"}
        self.addCleanup(self.host.stop)

    def test_empty_or_invalid_manifest_cannot_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests/scripts").mkdir(parents=True)
            for index, gates in enumerate([[], {}, [{"name": "empty", "command": ""}],
                                           [{"name": "same", "command": "true"}] * 2]):
                (root / "tests/scripts/quality-gates.json").write_text(json.dumps(gates))
                with patch.object(runner, "ROOT", root), \
                     patch("sys.argv", ["quality-gate.py", "--output", str(root / str(index))]):
                    with self.assertRaises(ValueError):
                        runner.main()

    def exercise(self, codes, changed=False, cleanup_code=0, environment=None):
        gate_codes = iter(codes)

        def execute(command, **kwargs):
            code = cleanup_code if command == ["cargo", "llvm-cov", "clean", "--workspace"] else next(gate_codes)
            return subprocess.CompletedProcess(command, code)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests/scripts").mkdir(parents=True)
            (root / "tests/scripts/quality-gates.json").write_text(json.dumps([
                {"name": "first", "command": "true"}, {"name": "second", "command": "true"}]))
            output = root / "evidence"
            with patch.dict(runner.os.environ, environment or {}), \
                 patch.object(runner, "ROOT", root), patch.object(runner, "capture", return_value="test"), \
                 patch.object(runner.subprocess, "check_output", return_value=b""), \
                 patch.object(runner, "source_fingerprint", side_effect=[{}, {"changed": "hash"} if changed else {}]), \
                 patch.object(runner.subprocess, "run", side_effect=execute) as run, \
                 patch("sys.argv", ["quality-gate.py", "--output", str(output)]):
                result = runner.main()
                report = json.loads((output / "summary.json").read_text())
                self.assertEqual(run.call_count, len(codes) + sum(code == 0 for code in codes))
                with self.assertRaises(FileExistsError):
                    runner.main()
                calls = list(run.call_args_list)
            return result, report, calls

    def test_success_requires_every_gate(self):
        result, report, _ = self.exercise([0, 0])
        self.assertEqual(result, 0)
        self.assertEqual(report["status"], "PASS")
        self.assertEqual([g["status"] for g in report["gates"]], ["PASS", "PASS"])
        self.assertIn("01.log", report["artifact_sha256"])
        self.assertIn("01-cleanup.log", report["artifact_sha256"])

    def test_failure_retains_evidence_and_stops(self):
        result, report, calls = self.exercise([7])
        self.assertEqual(result, 1)
        self.assertEqual(report["status"], "FAIL")
        self.assertEqual([g["status"] for g in report["gates"]], ["FAIL", "NOT_RUN"])
        self.assertEqual(report["gates"][0]["exit_code"], 7)
        self.assertNotIn("cleanup_log", report["gates"][0])
        self.assertEqual(len(calls), 1)

    def test_successful_gates_share_one_coverage_cache(self):
        for target, coverage, expected_path in [
                ("normal-build", "coverage-build", "coverage-build"),
                ("normal-build", "", "normal-build/llvm-cov-target"),
                ("", "", "target/llvm-cov-target")]:
            with self.subTest(target=target, coverage=coverage):
                result, report, calls = self.exercise([0, 0], environment={
                    "CARGO_TARGET_DIR": target, "CARGO_LLVM_COV_TARGET_DIR": coverage})
                self.assertEqual(result, 0)
                expected = str(calls[0].kwargs["cwd"] / expected_path)
                self.assertEqual(report["build_environment"]["CARGO_LLVM_COV_TARGET_DIR"], expected)
                self.assertEqual([call.args[0][0] for call in calls], ["bash", "cargo", "bash", "cargo"])
                for call in calls:
                    self.assertEqual(call.kwargs["env"]["CARGO_LLVM_COV_TARGET_DIR"], expected)
                self.assertTrue(all(gate["cleanup_exit_code"] == 0 for gate in report["gates"]))

    def test_cleanup_failure_blocks_later_gates_and_retains_logs(self):
        result, report, _ = self.exercise([0], cleanup_code=9)
        self.assertEqual(result, 1)
        self.assertEqual(report["status"], "FAIL")
        self.assertEqual([g["status"] for g in report["gates"]], ["PASS", "NOT_RUN"])
        self.assertEqual(report["gates"][0]["cleanup_exit_code"], 9)
        self.assertIn("Coverage cache cleanup", report["error"])
        self.assertIn("01-cleanup.log", report["artifact_sha256"])

    def test_source_changes_invalidate_successful_commands(self):
        result, report, _ = self.exercise([0, 0], changed=True)
        self.assertEqual(result, 1)
        self.assertEqual(report["status"], "FAIL")
        self.assertIn("Source changed", report["error"])
