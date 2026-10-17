"""Guard #285's pure planning/CG-10 boundaries alongside behavioral Rust proof."""
from pathlib import Path
import json
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ContextPlanningBoundary(unittest.TestCase):
    def test_no_acquisition_or_provider_implementation_in_planning(self):
        for path in ("crates/gateway-domain/src/context_planning.rs",
                     "crates/gateway-application/src/context_planning.rs",
                     "crates/gateway-context/src/planning.rs"):
            source = (ROOT / path).read_text()
            for forbidden in ("std::fs", "std::net", "std::process", "reqwest::",
                              "tokio::", "gateway_daemon::", "openai::",
                              ".retrieve(", ".execute(", ".invoke("):
                self.assertNotIn(forbidden, source, (path, forbidden))

    def test_canonical_types_have_one_owner(self):
        for name in ("ContextRequirementsIR", "ContextPlanIR"):
            owners = [str(p.relative_to(ROOT)) for p in (ROOT / "crates").glob("*/src/**/*.rs")
                      if "pub struct " + name + " {" in p.read_text()]
            self.assertEqual(owners, ["crates/gateway-domain/src/context_planning.rs"])
        context = (ROOT / "crates/gateway-context/src/planning.rs").read_text()
        self.assertIn("context_source_rejection(", context)
        self.assertIn("ContextCompiler", context)
        self.assertNotIn("gateway_application", context)

    def test_per_file_coverage_gate_is_part_of_release(self):
        gates = json.loads((ROOT / "scripts/quality-gates.json").read_text())
        self.assertEqual(len([g for g in gates if "check-context-planning-coverage.py" in g["command"]]), 1)
