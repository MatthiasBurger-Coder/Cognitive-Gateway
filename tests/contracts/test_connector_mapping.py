"""Independent structural and SHA-256 golden vectors for discovery mapping v1."""
import copy
import hashlib
import json
from pathlib import Path
import unittest
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/connector-mapping-v1'


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


class ConnectorMappingTests(unittest.TestCase):
    def test_strict_structural_contract(self):
        schema = json.loads((ROOT / 'schemas/connector-mapping-v1.schema.json').read_text())
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        doc = json.loads((FIXTURES / 'mapping.json').read_text())
        validator.validate(doc)
        for field, value in [('class', 'UNKNOWN'), ('side_effects', 'maybe'),
                             ('retry_safety', 'automatic'), ('token', 'CANARY'),
                             ('result_schema', {'type': 'object', 'properties': {},
                                                'additionalProperties': True}),
                             ('reference', 'mapping.v1\n')]:
            with self.subTest(field=field):
                bad = copy.deepcopy(doc)
                bad['records'][0][field] = value
                self.assertFalse(validator.is_valid(bad))
        bad = copy.deepcopy(doc)
        bad['records'][1]['source'] = None
        self.assertFalse(validator.is_valid(bad))

    def test_independent_complete_discovery_and_mapping_snapshot(self):
        manifest = json.loads((FIXTURES / 'manifest.json').read_text())
        wire = json.loads((FIXTURES / 'discovery.json').read_text())
        doc = json.loads((FIXTURES / 'mapping.json').read_text())
        doc['records'].sort(key=lambda r: r['reference'])
        self.assertEqual(manifest['digest'], digest(manifest['manifest']))
        entries, discovery = [], []
        for kind, key in [('tool', 'tools'), ('resource', 'resources'), ('prompt', 'prompts')]:
            for item in wire[key]:
                declared = next(e for e in manifest['manifest']['inventory']
                                if e['kind'] == kind and e['name'] == item['name'])
                self.assertEqual(declared['schema_digest'], digest({
                    'discovery_schema_version': '1.0', 'kind': kind, 'entry': item}))
                identity = item['uri'] if kind == 'resource' else item['name']
                discovery.append({'kind': kind, 'identity': identity, 'entry': item})
                if kind == 'prompt':
                    continue
                record = next(r for r in doc['records'] if r['kind'] == kind and r['name'] == item['name'])
                arguments = item['inputSchema'] if kind == 'tool' else {
                    'type': 'object', 'properties': {}, 'additionalProperties': False}
                result = record['result_schema'] if kind == 'tool' else {
                    'resource_result_version': '1.0', 'content': 'text_or_blob', 'uri': identity}
                entries.append({'mapping': record, 'wire_identity': identity,
                                'argument_schema_digest': digest(arguments),
                                'result_schema_digest': digest(result)})
        audit = {'inventory_version': '1.0', 'connector': 'example.records', 'scope': 'scope-one',
                 'manifest_digest': manifest['digest'], 'protocol': '2025-11-25',
                 'mapping_digest': digest(doc), 'discovery': sorted(discovery, key=canonical),
                 'templates': sorted(wire['templates'], key=lambda t: t['uriTemplate']),
                 'entries': sorted(entries, key=lambda e: e['mapping']['capability'])}
        self.assertEqual((FIXTURES / 'snapshot.json').read_bytes(), canonical(audit))
        self.assertEqual((FIXTURES / 'revision.txt').read_text().strip(), digest(audit))
