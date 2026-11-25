from argparse import Namespace
import importlib.util
import json
import re
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


class ShardTests(unittest.TestCase):
    def setUp(self):
        self.gates = runner.load_gates()
        self.plan = runner.shard_plan(self.gates)

    def test_shards_partition_the_manifest_and_keep_shared_artifacts_together(self):
        assignments = {index: shard for shard, indices in self.plan.items() for index in indices}
        self.assertEqual(sorted(assignments), list(range(len(self.gates))))
        self.assertEqual(sum(map(len, self.plan.values())), len(self.gates))
        self.assertEqual(len(self.plan), 4)
        artifact_users = {}
        for index, gate in enumerate(self.gates):
            for artifact in re.findall(r"\$(?:EVIDENCE_DIR|RUNNER_TEMP)/[^\s\"\\]+", gate["command"]):
                artifact_users.setdefault(artifact, set()).add(assignments[index])
        self.assertTrue(all(len(shards) == 1 for shards in artifact_users.values()), artifact_users)

    def test_shard_runs_only_assigned_commands_and_retains_global_log_numbers(self):
        host = type("Host", (), {"environment": {}, "report": {}})()
        shard = "models"
        indices = self.plan[shard]
        with (
            tempfile.TemporaryDirectory() as directory,
            patch.object(runner, "capture", return_value="candidate"),
            patch.object(runner, "source_fingerprint", return_value={}),
            patch.object(runner.subprocess, "check_output", return_value=b""),
            patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as command,
            patch.dict(runner.os.environ, {"CG_415_DIAGNOSTIC_TAIL": ""}),
        ):
            output = Path(directory) / "evidence"
            args = Namespace(shard=shard, output=output)
            self.assertEqual(runner.run(args, host), 0)
            actual = [call.args[0][-1] for call in command.call_args_list if call.args[0][0] == "bash"]
            self.assertEqual(actual, [self.gates[index]["command"] for index in indices])
            report = json.loads((output / "summary.json").read_text())
            self.assertEqual(report["shard"], shard)
            self.assertEqual([entry["log"] for entry in report["gates"]],
                             [f"{index + 1:02d}.log" for index in indices])

    def create_shard_reports(self, root):
        parts = root / "parts"
        for shard, indices in self.plan.items():
            directory = parts / f"quality-shard-{shard}"
            directory.mkdir(parents=True)
            (directory / "worktree.patch").touch()
            (directory / "cg03-host.json").write_text(json.dumps({"host": shard}))
            results = []
            for index in indices:
                log = f"{index + 1:02d}.log"
                cleanup_log = f"{index + 1:02d}-cleanup.log"
                for file in (log, cleanup_log):
                    (directory / file).touch()
                results.append({**self.gates[index], "status": "PASS", "exit_code": 0,
                                "cleanup_exit_code": 0, "log": log, "cleanup_log": cleanup_log})
            report = {"schema_version": 1, "scope": f"{runner.SCOPE}-shard", "shard": shard,
                      "status": "PASS", "revision": "candidate", "worktree_status": "",
                      "source_sha256": {"Cargo.toml": "digest"}, "toolchain": {"rustc": "test"},
                      "started_at": "2026-11-25T00:00:00+00:00",
                      "finished_at": "2026-11-25T01:00:00+00:00",
                      "gates": results, "build_environment": {},
                      "artifact_sha256": runner.artifact_hashes(directory)}
            (directory / "summary.json").write_text(json.dumps(report))
        return parts

    def collect(self, parts, output):
        with patch.object(runner, "capture", side_effect=lambda *args: "candidate" if "rev-parse" in args else ""),              patch.object(runner, "source_fingerprint", return_value={"Cargo.toml": "digest"}):
            return runner.collect(parts, output)

    def test_collect_reconstructs_complete_manifest_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "combined"
            parts = self.create_shard_reports(root)
            self.assertEqual(self.collect(parts, output), 0)
            report = json.loads((output / "summary.json").read_text())
            self.assertEqual(report["status"], "PASS")
            self.assertEqual(report["scope"], runner.SCOPE)
            self.assertEqual([{key: entry[key] for key in ("name", "command")} for entry in report["gates"]], self.gates)
            self.assertEqual(report["artifact_sha256"], runner.artifact_hashes(output))

    def test_collect_rejects_missing_shard_wrong_revision_tampering_and_failed_gate(self):
        for defect, message in (
            ("missing", "Missing or unexpected"),
            ("revision", "source binding mismatch"),
            ("artifact", "artifact digest mismatch"),
            ("gate", "missing successful result"),
            ("duplicate", "gate inventory mismatch"),
        ):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                output = root / "combined"
                parts = self.create_shard_reports(root)
                shard = parts / "quality-shard-foundation"
                summary = shard / "summary.json"
                report = json.loads(summary.read_text())
                if defect == "missing":
                    shard.rename(root / "missing")
                elif defect == "artifact":
                    (shard / "01.log").write_text("modified")
                else:
                    if defect == "revision":
                        report["revision"] = "different"
                    elif defect == "gate":
                        report["gates"][0]["cleanup_exit_code"] = 1
                    else:
                        report["gates"].append(report["gates"][0])
                    summary.write_text(json.dumps(report))
                with self.assertRaisesRegex(ValueError, message):
                    self.collect(parts, output)
                self.assertFalse((output / "summary.json").exists())
