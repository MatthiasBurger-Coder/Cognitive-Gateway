"""Offline structural schema conformance; Rust additionally enforces semantic rules."""
import hashlib
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/connector-manifest-v1'


class ConnectorManifestSchemaTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        schema = json.loads((ROOT / 'schemas/connector-manifest-v1.schema.json').read_text())
        Draft202012Validator.check_schema(schema)
        cls.validator = Draft202012Validator(schema)

    def test_valid_examples_and_independent_digest_vectors(self):
        for name in ('valid-stdio.json', 'valid-http.json'):
            with self.subTest(name=name):
                value = json.loads((FIXTURES / name).read_text())
                self.validator.validate(value)
                canonical = json.dumps(value['manifest'], sort_keys=True, separators=(',', ':')).encode()
                self.assertEqual(value['digest'], hashlib.sha256(canonical).hexdigest())

    def test_hostile_fixtures_record_structural_vs_semantic_boundary(self):
        cases = json.loads((FIXTURES / 'invalid-cases.json').read_text())
        for case in cases:
            with self.subTest(name=case['file']):
                value = json.loads((FIXTURES / case['file']).read_text())
                self.assertEqual(self.validator.is_valid(value), case['schema_valid'])

    def test_ascii_patterns_reject_trailing_newline(self):
        for path in (("manifest", "connector_id"), ("digest",)):
            value = json.loads((FIXTURES / 'valid-stdio.json').read_text())
            target = value
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] += '\n'
            self.assertFalse(self.validator.is_valid(value))

    def test_nested_unknown_fields_and_missing_required_fields(self):
        for key in ('protocol', 'profiles', 'health', 'compatibility', 'version', 'server_version'):
            with self.subTest(key=key):
                value = json.loads((FIXTURES / 'valid-stdio.json').read_text())
                value['manifest'][key]['token'] = 'canary-secret'
                self.assertFalse(self.validator.is_valid(value))
                del value['manifest'][key]
                self.assertFalse(self.validator.is_valid(value))


if __name__ == '__main__':
    unittest.main()
