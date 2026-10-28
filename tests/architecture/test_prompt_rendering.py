"""Renderer placement, explicit accounting and measured qualification stay enforced."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class PromptRenderingArchitecture(unittest.TestCase):
    def test_runtime_formats_remain_in_outer_adapter(self):
        for path in ("crates/gateway-domain/src/prompt_rendering.rs",
                     "crates/gateway-application/src/prompt_rendering.rs"):
            source = (ROOT / path).read_text()
            for forbidden in ("ChatMessagesV1", "BlockMessagesV1", "gateway_daemon::",
                              "reqwest::", "std::net", "std::fs"):
                self.assertNotIn(forbidden, source)
        adapter = (ROOT / "crates/gateway-daemon/src/prompt_renderers.rs").read_text()
        self.assertIn("impl PromptRendererPort", adapter)
        for forbidden in ("std::fs", "std::net", "reqwest::", "Command::"):
            self.assertNotIn(forbidden, adapter)

    def test_measured_coverage_gate_rejects_missing_and_undercovered_files(self):
        spec = importlib.util.spec_from_file_location(
            "prompt_coverage", ROOT / "tests/scripts/check-prompt-rendering-coverage.py")
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        gate.self_test()
        self.assertEqual(len(gate.EXPECTED), 3)
        configured = next(g for g in json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
                          if g["name"].startswith("EPIC-06.04"))
        for required in ("cargo llvm-cov", "--test prompt_rendering", "--locked",
                         "--test invocation_host", "--test model_adapters",
                         "check-prompt-rendering-coverage.py"):
            self.assertIn(required, configured["command"])


if __name__ == "__main__":
    unittest.main()
