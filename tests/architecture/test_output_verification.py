"""Verification stays in the core and retains a measured qualification gate."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class OutputVerificationArchitecture(unittest.TestCase):
    def test_verification_has_no_effect_or_provider_dependency(self):
        for path in ("crates/gateway-domain/src/grounding.rs",
                     "crates/gateway-application/src/grounding.rs",
                     "crates/gateway-domain/src/output_parsing.rs",
                     "crates/gateway-application/src/output_parsing.rs",
                     "crates/gateway-domain/src/output_verification.rs",
                     "crates/gateway-application/src/output_verification.rs"):
            source = (ROOT / path).read_text()
            for forbidden in ("std::process", "std::net", "std::fs", "reqwest::",
                              "gateway_daemon::", "openai::", "anthropic::",
                              "dispatch_invocation_ir", "ModelInvocationPort"):
                self.assertNotIn(forbidden, source)

    def test_ci_requires_measured_per_file_coverage(self):
        configured = next(g for g in json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
                          if g["name"].startswith("EPIC-06.06"))
        for required in ("cargo llvm-cov", "--test output_verification", "--test prompt_ir",
                         "--locked", "--self-test", "check-output-verification-coverage.py"):
            self.assertIn(required, configured["command"])
        spec = importlib.util.spec_from_file_location(
            "output_coverage", ROOT / "tests/scripts/check-output-verification-coverage.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        gate.self_test()

    def test_parsing_ci_requires_measured_coverage(self):
        configured = next(g for g in json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
                          if g["name"].startswith("EPIC-06.05"))
        for required in ("cargo llvm-cov", "--test output_verification", "--locked",
                         "--self-test", "check-output-parsing-coverage.py"):
            self.assertIn(required, configured["command"])
        spec = importlib.util.spec_from_file_location(
            "parsing_coverage", ROOT / "tests/scripts/check-output-parsing-coverage.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        gate.self_test()

    def test_grounding_ci_requires_measured_coverage(self):
        configured = next(g for g in json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
                          if g["name"].startswith("EPIC-06.07"))
        for required in ("cargo llvm-cov", "--test output_verification", "--locked",
                         "--self-test", "check-grounding-coverage.py"):
            self.assertIn(required, configured["command"])
        spec = importlib.util.spec_from_file_location(
            "grounding_coverage", ROOT / "tests/scripts/check-grounding-coverage.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        gate.self_test()
