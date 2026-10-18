"""Invocation resilience owns accounting, without provider or lifecycle authority."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class InvocationResilienceArchitecture(unittest.TestCase):
    def test_no_provider_or_effect_dependencies(self):
        for layer in ("gateway-domain", "gateway-application"):
            source = (ROOT / "crates" / layer / "src/invocation_resilience.rs").read_text()
            for forbidden in ("reqwest::", "std::process", "std::net", "std::fs",
                              "openai::", "anthropic::", "gateway_daemon::"):
                self.assertNotIn(forbidden, source)

    def test_existing_lifecycle_admission_and_parsing_owners_are_reused(self):
        source = (ROOT / "crates/gateway-application/src/invocation_resilience.rs").read_text()
        for owner in ("closed_loop::LoopDecision", "RequestContext", "validate_current",
                      "parse_invocation_output", "RenderedInvocationIr"):
            self.assertIn(owner, source)

    def test_ci_enforces_measured_coverage_and_denies_invalid_evidence(self):
        gate = next(g for g in json.loads((ROOT / "scripts/quality-gates.json").read_text())
                    if g["name"].startswith("EPIC-06.08"))
        for command in ("cargo llvm-cov", "--test invocation_resilience", "--locked",
                        "check-invocation-resilience-coverage.py", "--self-test"):
            self.assertIn(command, gate["command"])
        spec = importlib.util.spec_from_file_location(
            "resilience_coverage", ROOT / "scripts/check-invocation-resilience-coverage.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.self_test()
