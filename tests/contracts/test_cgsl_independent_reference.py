"""Offline checks of the separate valid-source witness, never runtime authority."""
import importlib.util
import json
from pathlib import Path
import unittest
from jsonschema.exceptions import ValidationError

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("cgsl_reference", ROOT / "tests/conformance/cgsl_reference.py")
REFERENCE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REFERENCE)


class IndependentConformanceTests(unittest.TestCase):
    def test_frozen_valid_source_vectors_without_rust(self):
        corpus = ROOT / "tests/fixtures/cgsl-conformance-v1"
        manifest = json.loads((corpus / "manifest.json").read_text())
        count = 0
        for case in manifest["cases"]:
            if "canonical_ir" not in case:
                continue
            result = REFERENCE.compile_source((corpus / case["source"]).read_text())
            self.assertEqual(result["canonical_ir"], (corpus / case["canonical_ir"]).read_text().strip())
            self.assertEqual(result["sha256"], case["sha256"])
            count += 1
        self.assertEqual(count, 4)

    def test_invalid_operands_never_produce_a_task(self):
        original = (ROOT / "examples/cgsl-v1/minimal.cgsl").read_text()
        mutations = [
            lambda d: d.update(cgsl="2.0"),
            lambda d: d.update(provider="forbidden"),
            lambda d: d["output"]["schema"].update(scope="foreign"),
            lambda d: d["verification"].update(checks=[]),
            lambda d: d.update(knowledge_gap=[{}]),
            lambda d: d.update(input=[{"role":"x","value":{"kind":"LITERAL", "value":{"kind":"SET","value":[{"kind":"INTEGER","value":1},{"kind":"STRING","value":"one"}]}}}]),
        ]
        for mutate in mutations:
            value = json.loads(original)
            mutate(value)
            with self.assertRaises((ValueError, ValidationError)):
                REFERENCE.compile_source(json.dumps(value))
        with self.assertRaises(ValueError):
            REFERENCE.compile_source(original.replace('"cgsl": "1.0"', '"cgsl": "1.0", "cgsl": "1.0"'))


if __name__ == "__main__":
    unittest.main()
