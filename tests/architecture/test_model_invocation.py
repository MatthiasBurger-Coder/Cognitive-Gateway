"""Keep model contracts independent of provider payloads and preserve CI coverage."""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ModelInvocationArchitecture(unittest.TestCase):
    def test_contracts_have_no_provider_or_network_dependencies(self):
        for relative in ("crates/gateway-domain/src/model_invocation.rs",
                         "crates/gateway-application/src/model_invocation.rs"):
            source = (ROOT / relative).read_text()
            for forbidden in ("reqwest::", "openai::", "anthropic::", "ollama::",
                              "std::net", "serde_json::Value", "pub prompt:"):
                self.assertNotIn(forbidden, source)
        # Full normalized dependency graph is checked by check-architecture.sh.
        self.assertNotIn("gateway-application", (ROOT / "crates/gateway-domain/Cargo.toml").read_text())

    def test_measured_contract_coverage_is_part_of_release_gate(self):
        gates = json.loads((ROOT / "scripts/quality-gates.json").read_text())
        gate = next(g for g in gates if g["name"].startswith("EPIC-06.01"))
        self.assertIn("cargo llvm-cov", gate["command"])
        self.assertIn("--test model_invocation", gate["command"])
        self.assertIn("check-model-invocation-coverage.py", gate["command"])
        self.assertIn("--self-test", gate["command"])
