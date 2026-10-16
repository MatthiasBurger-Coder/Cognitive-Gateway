"""Independent Draft 2020-12 checks for EPIC-05.06 projections."""
import copy
import json
from pathlib import Path
import unittest
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/semantic-output-v1'


class SemanticOutputContractTests(unittest.TestCase):
    def test_definition_fixtures_and_closed_provider_boundary(self):
        schema = json.loads((ROOT / 'schemas/semantic-output-contract.schema.json').read_text())
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        for name in ['output', 'verification']:
            value = json.loads((FIXTURES / f'{name}.json').read_text())
            validator.validate(value)
            for field in value:
                bad = copy.deepcopy(value)
                del bad[field]
                self.assertFalse(validator.is_valid(bad))
            for version in ['1.1', '2.0']:
                bad = dict(value, schema_version=version)
                self.assertFalse(validator.is_valid(bad))
            self.assertFalse(validator.is_valid(dict(value, provider='unsupported')))

    def test_result_projection(self):
        schema = json.loads((ROOT / 'schemas/artifact-inspection-output.schema.json').read_text())
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        validator.validate(json.loads((FIXTURES / 'result-valid.json').read_text()))
        self.assertFalse(validator.is_valid(json.loads((FIXTURES / 'result-invalid.json').read_text())))
        for value in [None, {}, {'summary': 'x', 'scores': None},
                      {'summary': 'x', 'scores': [], 'extra': 1},
                      {'summary': 'x', 'scores': [], 'approved': None}]:
            self.assertFalse(validator.is_valid(value))
        for value in [{'summary': 'x', 'scores': []},
                      {'summary': 'x', 'scores': [0.1], 'approved': True}]:
            validator.validate(value)


if __name__ == '__main__':
    unittest.main()
