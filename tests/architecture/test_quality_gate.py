import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("runner", ROOT / "scripts/quality-gate.py")
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
            (root / "scripts").mkdir()
            for index, gates in enumerate([[], {}, [{"name": "empty", "command": ""}],
                                           [{"name": "same", "command": "true"}] * 2]):
                (root / "scripts/quality-gates.json").write_text(json.dumps(gates))
                with patch.object(runner, "ROOT", root), \
                     patch("sys.argv", ["quality-gate.py", "--output", str(root / str(index))]):
                    with self.assertRaises(ValueError):
                        runner.main()

    def exercise(self, codes, changed=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / "scripts/quality-gates.json").write_text(json.dumps([
                {"name": "first", "command": "true"}, {"name": "second", "command": "true"}]))
            output = root / "evidence"
            with patch.object(runner, "ROOT", root), patch.object(runner, "capture", return_value="test"), \
                 patch.object(runner.subprocess, "check_output", return_value=b""), \
                 patch.object(runner, "source_fingerprint", side_effect=[{}, {"changed": "hash"} if changed else {}]), \
                 patch.object(runner.subprocess, "run", side_effect=[subprocess.CompletedProcess([], c) for c in codes]) as run, \
                 patch("sys.argv", ["quality-gate.py", "--output", str(output)]):
                result = runner.main()
                report = json.loads((output / "summary.json").read_text())
                self.assertEqual(run.call_count, len(codes))
                with self.assertRaises(FileExistsError):
                    runner.main()
            return result, report

    def test_success_requires_every_gate(self):
        result, report = self.exercise([0, 0])
        self.assertEqual(result, 0)
        self.assertEqual(report["status"], "PASS")
        self.assertEqual([g["status"] for g in report["gates"]], ["PASS", "PASS"])
        self.assertIn("01.log", report["artifact_sha256"])

    def test_failure_retains_evidence_and_stops(self):
        result, report = self.exercise([7])
        self.assertEqual(result, 1)
        self.assertEqual(report["status"], "FAIL")
        self.assertEqual([g["status"] for g in report["gates"]], ["FAIL", "NOT_RUN"])
        self.assertEqual(report["gates"][0]["exit_code"], 7)

    def test_source_changes_invalidate_successful_commands(self):
        result, report = self.exercise([0, 0], changed=True)
        self.assertEqual(result, 1)
        self.assertEqual(report["status"], "FAIL")
        self.assertIn("Source changed", report["error"])
