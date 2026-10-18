"""Invocation IR keeps producer ownership and enforced measured qualification."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class PromptIrArchitecture(unittest.TestCase):
    def test_ir_uses_existing_owners_without_provider_or_acquisition_dependencies(self):
        domain = (ROOT / "crates/gateway-domain/src/prompt_ir.rs").read_text()
        application = (ROOT / "crates/gateway-application/src/prompt_ir.rs").read_text()
        for source in (domain, application):
            for forbidden in ("reqwest::", "openai::", "anthropic::", "ollama::",
                              "std::net", "std::fs", "gateway_daemon::"):
                self.assertNotIn(forbidden, source)
        self.assertNotIn("gateway_context::", domain)
        for owner in ("PlannedContextHandoff", "PreparedModelInvocation",
                      "render_invocation", "validate_references"):
            self.assertIn(owner, application)

    def test_measured_coverage_and_gate_rejection_are_part_of_ci(self):
        gates = json.loads((ROOT / "scripts/quality-gates.json").read_text())
        gate = next(g for g in gates if g["name"].startswith("EPIC-06.03"))
        for required in ("cargo llvm-cov", "--test prompt_ir", "--self-test",
                         "check-prompt-ir-coverage.py"):
            self.assertIn(required, gate["command"])
        spec = importlib.util.spec_from_file_location(
            "prompt_ir_coverage", ROOT / "scripts/check-prompt-ir-coverage.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.self_test()
