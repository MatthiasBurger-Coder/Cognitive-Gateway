"""Prove release admission rejects missing, altered or failed evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("cg30", ROOT / "scripts/qualify-cg30.py")
cg30 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cg30)


class QualificationTests(unittest.TestCase):
    def test_missing_invalid_measurements_fail_closed(self):
        for bundle in ({}, {"evidence": {}, "payload_sha256": ""},
                       {"evidence": {}, "payload_sha256": "0" * 64}):
            with self.subTest(bundle=bundle), self.assertRaises((ValueError, KeyError)):
                cg30.validate_metrics(bundle)
        for invalid in (None, True, -1, 1.0, "1"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                cg30.count(invalid)

    def fixture(self, root):
        manifest = json.loads((ROOT / "scripts/quality-gates.json").read_text())
        files = {name: "{}" for name in ("cg30-qualification.json", "cg16-coverage.json", "cg23-evaluation.json",
                                       "cg24-promotion.json", "cg25-reflex.json", "cg27-fixture-benchmark.json", "cg28-learning.json")}
        for name, content in files.items():
            (root / name).write_text(content)
        return {"status": "PASS", "worktree_status": "", "revision": "candidate",
                "gates": [{**gate, "status": "PASS", "exit_code": 0} for gate in manifest],
                "source_sha256": {"Cargo.lock": cg30.sha256(ROOT / "Cargo.lock")},
                "artifact_sha256": {name: cg30.sha256(root / name) for name in files},
                "toolchain": {"rustc": "fixture-toolchain"}}

    def test_release_requires_exact_clean_complete_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            good = self.fixture(root)
            evidence = {"suite": "CG-30-v1", "classification": {}, "routing": {}, "performance": {}}
            with patch.object(cg30.subprocess, "check_output", side_effect=lambda args, **kw: "candidate\n" if args[1] == "rev-parse" else ""), \
                 patch.object(cg30, "validate_metrics", return_value=evidence):
                (root / "summary.json").write_text(json.dumps(good))
                self.assertEqual(cg30.qualify(root)["status"], "QUALIFIED_FIXTURE_SCOPE")
                for mutate in (
                    lambda s: s.update(status="FAIL"),
                    lambda s: s.update(worktree_status=" M Cargo.lock"),
                    lambda s: s.update(revision="other"),
                    lambda s: s["gates"].pop(),
                    lambda s: s["gates"][0].update(status="FAIL"),
                    lambda s: s["gates"][0].update(exit_code=1),
                    lambda s: s["source_sha256"].update({"Cargo.lock": "bad"}),
                    lambda s: s["artifact_sha256"].pop("cg16-coverage.json"),
                    lambda s: s["artifact_sha256"].update({"cg30-qualification.json": "bad"}),
                ):
                    candidate = copy.deepcopy(good)
                    mutate(candidate)
                    (root / "summary.json").write_text(json.dumps(candidate))
                    with self.assertRaises(ValueError):
                        cg30.qualify(root)
                (root / "summary.json").write_text(json.dumps(good))
                (root / "cg30-qualification.json").write_text("altered")
                with self.assertRaises(ValueError):
                    cg30.qualify(root)

    def test_current_dirty_worktree_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "summary.json").write_text(json.dumps(self.fixture(root)))
            with patch.object(cg30.subprocess, "check_output", side_effect=["candidate\n", "?? new-source.rs\n"]), self.assertRaises(ValueError):
                cg30.qualify(root)
