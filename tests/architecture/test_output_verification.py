"""Verification stays in the core and retains a measured qualification gate."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class OutputVerificationArchitecture(unittest.TestCase):
    def test_verification_has_no_effect_or_provider_dependency(self):
        for path in ("crates/gateway-domain/src/output_verification.rs",
                     "crates/gateway-application/src/output_verification.rs"):
            source = (ROOT / path).read_text()
            for forbidden in ("std::process", "std::net", "std::fs", "reqwest::",
                              "gateway_daemon::", "openai::", "anthropic::",
                              "dispatch_invocation_ir", "ModelInvocationPort"):
                self.assertNotIn(forbidden, source)

    def test_ci_requires_measured_per_file_coverage(self):
        configured = next(g for g in json.loads((ROOT / "scripts/quality-gates.json").read_text())
                          if g["name"].startswith("EPIC-06.06"))
        for required in ("cargo llvm-cov", "--test output_verification", "--test prompt_ir",
                         "--locked", "--self-test", "check-output-verification-coverage.py"):
            self.assertIn(required, configured["command"])
        spec = importlib.util.spec_from_file_location(
            "output_coverage", ROOT / "scripts/check-output-verification-coverage.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        gate.self_test()
