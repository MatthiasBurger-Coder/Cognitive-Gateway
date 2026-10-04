"""EPIC-04.03 local MCP framing remains outer infrastructure."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LocalMcpBoundaryTests(unittest.TestCase):
    def test_protocol_modules_remain_outside_inner_crates(self):
        self.assertTrue((ROOT / "crates/gateway-daemon/src/local_mcp/mod.rs").is_file())
        for crate in (ROOT / "crates").iterdir():
            if crate.name == "gateway-daemon":
                continue
            for source in (crate / "src").rglob("*.rs"):
                text = source.read_text()
                for forbidden in ("local_mcp::", "mod local_mcp", "StdioTransport", "LaunchBinding", "cg-mcp"):
                    self.assertNotIn(forbidden, text, str(source))

    def test_framing_adapter_has_no_application_or_provider_imports(self):
        source = (ROOT / "crates/gateway-daemon/src/local_mcp/transport.rs").read_text()
        for forbidden in ("gateway_application", "gateway_domain", "openai", "codex", "postgres"):
            self.assertNotIn(forbidden, source)

    def test_coverage_gate_rejects_missing_and_undercovered_adapter_files(self):
        spec = importlib.util.spec_from_file_location("local_mcp_coverage", ROOT / "scripts/check-local-mcp-coverage.py")
        coverage = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(coverage)
        coverage.self_test()
        report = {"data": [{"files": [
            {"filename": str(ROOT / path), "summary": {"lines": {"count": 100, "covered": 95}}}
            for path in coverage.EXPECTED]}]}
        self.assertEqual(len(coverage.check(report)), 5)
        report["data"][0]["files"][0]["summary"]["lines"]["covered"] = 94
        with self.assertRaises(ValueError):
            coverage.check(report)
