"""Keep #286 canonical selection pure and its actual CG-10 producer gate enforced."""
from pathlib import Path
import json
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ExampleSelectionBoundary(unittest.TestCase):
    def test_no_provider_or_acquisition_in_canonical_producers(self):
        for path in ("crates/gateway-domain/src/example_selection.rs",
                     "crates/gateway-application/src/example_selection.rs",
                     "crates/gateway-context/src/planning.rs"):
            source = (ROOT / path).read_text()
            for forbidden in ("std::fs", "std::net", "std::process", "reqwest::",
                              "tokio::", "gateway_daemon::", "openai::",
                              ".retrieve(", ".execute(", ".invoke("):
                self.assertNotIn(forbidden, source, (path, forbidden))

    def test_canonical_contracts_have_one_domain_owner(self):
        for name in ("ExampleCandidate", "ExampleRequirement", "ExampleSet"):
            owners = [str(p.relative_to(ROOT)) for p in (ROOT / "crates").glob("*/src/**/*.rs")
                      if "pub struct " + name + " {" in p.read_text()]
            self.assertEqual(owners, ["crates/gateway-domain/src/example_selection.rs"])
        context = (ROOT / "crates/gateway-context/src/planning.rs").read_text()
        self.assertIn("ContextSatisfaction::Examples", context)
        self.assertIn("handoff_planned_context_with_examples", context)
        self.assertNotIn("gateway_application", context)

    def test_producer_conformance_coverage_is_a_release_gate(self):
        gates = json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
        matches = [g for g in gates if "check-example-selection-coverage.py" in g["command"]]
        self.assertEqual(len(matches), 1)
        self.assertIn("--test example_selection --test context_planning", matches[0]["command"])
