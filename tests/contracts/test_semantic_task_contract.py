"""Independent JSON Schema checks for the SemanticTaskIR v1 wire fixtures."""
import copy
import hashlib
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/semantic-task-v1'


class SemanticTaskContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schema = json.loads((ROOT / 'schemas/semantic-task.schema.json').read_text())
        Draft202012Validator.check_schema(cls.schema)
        cls.validator = Draft202012Validator(cls.schema)
        cls.full = json.loads((FIXTURES / 'performance-analysis.json').read_text())

    def test_roundtrip_fixtures_match_schema_canonical_bytes_and_digest(self):
        for name in ['minimal', 'performance-analysis']:
            with self.subTest(fixture=name):
                value = json.loads((FIXTURES / f'{name}.json').read_text())
                self.validator.validate(value)
                canonical = (FIXTURES / f'{name}.canonical.json').read_bytes()
                self.assertEqual(canonical, json.dumps(value, sort_keys=True,
                                 separators=(',', ':')).encode())
                self.assertEqual(hashlib.sha256(canonical).hexdigest(),
                                 (FIXTURES / f'{name}.sha256').read_text().strip())

    def test_missing_fields_unknown_versions_provider_fields_and_candidates(self):
        for field in self.schema['required']:
            value = copy.deepcopy(self.full)
            del value[field]
            self.assertTrue(list(self.validator.iter_errors(value)), field)
        for version in ['2.0', '1.1', '0.9', 'v1', None]:
            value = copy.deepcopy(self.full)
            value['schema_version'] = version
            self.assertTrue(list(self.validator.iter_errors(value)))
        for field in ['model', 'provider', 'prompt', 'temperature', 'knowledge_gap', 'ambiguity']:
            value = copy.deepcopy(self.full)
            value[field] = 'unsupported'
            self.assertTrue(list(self.validator.iter_errors(value)))
        for field in ['target', 'goal', 'output_contract', 'verification_contract']:
            for replacement in [None, {'kind': 'UNRESOLVED'}, {'candidates': ['a', 'b']}]:
                value = copy.deepcopy(self.full)
                value[field] = replacement
                self.assertTrue(list(self.validator.iter_errors(value)))

    def test_nested_provider_configuration_and_invalid_verification_rejected(self):
        for field in ['target', 'goal', 'output_contract', 'verification_contract']:
            value = copy.deepcopy(self.full)
            value[field]['model'] = 'unsupported'
            self.assertTrue(list(self.validator.iter_errors(value)))
        for checks in [[], ['OUTPUT_SCHEMA_VALID'],
                       ['OUTPUT_SCHEMA_VALID', 'NO_UNRESOLVED_REFERENCE', 'ALL_CLAIMS_SUPPORTED']]:
            value = copy.deepcopy(self.full)
            value['verification_contract']['checks'] = checks
            self.assertTrue(list(self.validator.iter_errors(value)))


if __name__ == '__main__':
    unittest.main()
