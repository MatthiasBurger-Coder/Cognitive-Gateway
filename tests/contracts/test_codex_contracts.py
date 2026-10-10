"""Normative wire-contract tests; no MCP runtime or new CG authority is implemented."""
import copy
import hashlib
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[2]
SCHEMAS = ROOT / 'schemas/codex/v1'
FIXTURES = ROOT / 'tests/fixtures/codex-v1'


def load(path):
    return json.loads(path.read_text())


class CodexContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schemas = {p.name: load(p) for p in SCHEMAS.glob('*.schema.json')}
        registry = Registry().with_resources(
            (value['$id'], Resource.from_contents(value)) for value in cls.schemas.values())
        cls.validators = {name: Draft202012Validator(value, registry=registry)
                          for name, value in cls.schemas.items()}

    def valid(self, name, value):
        self.validators[name].validate(value)

    def invalid(self, name, value):
        self.assertTrue(list(self.validators[name].iter_errors(value)))

    def test_schemas_and_all_frozen_examples(self):
        for name, schema in self.schemas.items():
            with self.subTest(schema=name):
                Draft202012Validator.check_schema(schema)
        self.valid('catalog.schema.json', load(SCHEMAS / 'catalog.json'))
        for path in sorted(FIXTURES.glob('*.json')):
            with self.subTest(fixture=path.name):
                name = 'request' if '.request.' in path.name else 'resource' if '.resource.' in path.name else 'response'
                self.valid(name + '.schema.json', load(path))

    def test_qualification_goldens_match_independent_frozen_schemas(self):
        golden = ROOT / 'tests/fixtures/codex-qualification'
        self.valid('response.schema.json', load(golden / 'inspect.response.json'))
        envelope = load(FIXTURES / 'session.inspect.response.json')
        for scenario in load(golden / 'session-projections.json'):
            with self.subTest(operation=scenario['operation'], status=scenario['projection']['status']):
                response = copy.deepcopy(envelope)
                response.update(status='ok', operation=scenario['operation'], diagnostics=[],
                                result=scenario['projection'])
                self.valid('response.schema.json', response)

    def test_unknown_versions_fields_and_operations_fail_closed(self):
        for suffix in ['request', 'response', 'resource']:
            original = load(FIXTURES / ('assessment.resource.json' if suffix == 'resource'
                            else 'situation.assess.' + suffix + '.json'))
            for version in ['0.9', '1.1', '2.0', 1, None]:
                value = copy.deepcopy(original)
                value['schema_version'] = version
                self.invalid(suffix + '.schema.json', value)
            value = copy.deepcopy(original)
            value['provider_api_key'] = 'unsolicited'
            self.invalid(suffix + '.schema.json', value)
        original = load(FIXTURES / 'situation.inspect.request.json')
        for field in ['scope', 'execution', 'correlation', 'input']:
            value = copy.deepcopy(original)
            value[field]['unknown'] = True
            self.invalid('request.schema.json', value)
        for op in ['policy.grant', 'SITUATION.INSPECT', 'session.restart', '']:
            value = copy.deepcopy(original)
            value['operation'] = op
            self.invalid('request.schema.json', value)

    def test_operation_inputs_are_not_interchangeable(self):
        requests = [load(p) for p in FIXTURES.glob('*.request.json')]
        for original in requests:
            for other in requests:
                if original['operation'] == other['operation'] or original['input'] == other['input']:
                    continue
                value = copy.deepcopy(original)
                value['input'] = other['input']
                with self.subTest(operation=value['operation'], other=other['operation']):
                    self.invalid('request.schema.json', value)

    def test_typed_references_and_no_authority_flags(self):
        original = load(FIXTURES / 'session.approve.request.json')
        for key, value in [('approved', True), ('grant_permissions', ['mutate']), ('expected_revision', -1)]:
            request = copy.deepcopy(original)
            request['input'][key] = value
            self.invalid('request.schema.json', request)
        for key, value in [('digest', 'unbound'), ('contract', 'cg.intent'), ('contract_version', '')]:
            request = copy.deepcopy(original)
            request['input']['consent_record'][key] = value
            self.invalid('request.schema.json', request)
        request = copy.deepcopy(original)
        del request['input']['expected_revision']
        self.invalid('request.schema.json', request)

    def test_reference_scope_tokens_and_source_variants_are_strict(self):
        original = load(FIXTURES / 'situation.inspect.request.json')
        for token in ['', '../other', '/workspace', 'workspace/other']:
            request = copy.deepcopy(original)
            request['scope']['workspace_id'] = token
            self.invalid('request.schema.json', request)
        request = copy.deepcopy(original)
        request['input']['situation']['reference']['raw_secret'] = 'unsolicited'
        self.invalid('request.schema.json', request)
        request = copy.deepcopy(original)
        request['input']['situation']['document'] = {}
        self.invalid('request.schema.json', request)
        request = copy.deepcopy(original)
        del request['input']['situation']['reference']['contract_version']
        self.invalid('request.schema.json', request)

    def test_errors_cannot_carry_results_or_sensitive_messages(self):
        original = load(FIXTURES / 'CG_SCOPE_DENIED.response.json')
        for key, value in [('result', load(FIXTURES / 'situation.assess.response.json')['result']),
                           ('status', 'ok'), ('evidence', [load(FIXTURES / 'assessment.resource.json')['reference']])]:
            response = copy.deepcopy(original)
            response[key] = value
            self.invalid('response.schema.json', response)
        for field, value in [('message', 'raw rejected payload'), ('code', 'UNKNOWN'), ('retry', 'automatic')]:
            response = copy.deepcopy(original)
            response['diagnostics'][0][field] = value
            self.invalid('response.schema.json', response)

    def test_query_and_session_results_are_distinct(self):
        response = load(FIXTURES / 'situation.assess.response.json')
        response['result'] = load(FIXTURES / 'projection.running.response.json')['result']
        self.invalid('response.schema.json', response)
        response = load(FIXTURES / 'projection.running.response.json')
        response['result'] = load(FIXTURES / 'situation.assess.response.json')['result']
        self.invalid('response.schema.json', response)
        response = load(FIXTURES / 'situation.assess.response.json')
        response['result']['canonical_result']['contract'] = 'cg.execution-context'
        self.invalid('response.schema.json', response)

    def test_only_completed_sessions_have_verified_final_results(self):
        response = load(FIXTURES / 'projection.completed.response.json')
        response['result']['verified_final_result'] = None
        self.invalid('response.schema.json', response)
        response = load(FIXTURES / 'projection.running.response.json')
        response['result']['verified_final_result'] = load(FIXTURES / 'assessment.resource.json')['reference']
        self.invalid('response.schema.json', response)
        response = load(FIXTURES / 'projection.pending_consent.response.json')
        response['result']['pending'] = []
        self.invalid('response.schema.json', response)
        response = load(FIXTURES / 'projection.pending_consent.response.json')
        response['result']['pending'][0]['kind'] = 'clarification'
        self.invalid('response.schema.json', response)

    def test_provenance_keeps_revision_digest_sensitivity_and_lineage(self):
        resource = load(FIXTURES / 'assessment.resource.json')
        reference = resource['reference']
        provenance = {'reference': reference, 'source_id': 'synthetic-source',
                      'source_revision': 'fixture-1', 'freshness': 'unknown',
                      'sensitivity': 'NORMAL', 'lineage': [reference]}
        resource['provenance'] = [provenance]
        self.valid('resource.schema.json', resource)
        for field in provenance:
            changed = copy.deepcopy(resource)
            del changed['provenance'][0][field]
            self.invalid('resource.schema.json', changed)
        changed = copy.deepcopy(resource)
        changed['provenance'][0]['sensitivity'] = 'unclassified'
        self.invalid('resource.schema.json', changed)

    def test_catalog_covers_all_operations_without_runtime_claims(self):
        catalog = load(SCHEMAS / 'catalog.json')
        tools = catalog['tools']
        operations = self.schemas['request.schema.json']['properties']['operation']['enum']
        self.assertEqual([t['operation'] for t in tools], operations)
        self.assertEqual(len({t['name'] for t in tools}), len(tools))
        for tool in tools:
            mutate = tool['operation'].startswith('session.') and tool['operation'] != 'session.inspect'
            self.assertEqual(tool['classification'], 'mutate' if mutate else 'inspect')
            self.assertEqual(tool['annotations']['readOnlyHint'], not mutate)
            self.assertEqual(tool['annotations']['idempotentHint'], not mutate)
            self.assertEqual(tool['execution']['taskSupport'], 'forbidden')
            if tool['operation'].startswith('session.'):
                self.assertEqual(tool['availability'], 'unsupported')
                response = load(FIXTURES / (tool['operation'] + '.response.json'))
                self.assertEqual(response['status'], 'unsupported')
            request = load(FIXTURES / (tool['operation'] + '.request.json'))
            response = load(FIXTURES / (tool['operation'] + '.response.json'))
            self.assertEqual(request['scope'], response['scope'])
            self.assertEqual(request['correlation'], response['correlation'])

    def test_authoritative_payload_and_pinned_resource_are_preserved(self):
        path = ROOT / 'tests/fixtures/declarative-v0.1/assessment.json'
        canonical = load(path)
        response = load(FIXTURES / 'situation.assess.response.json')
        resource = load(FIXTURES / 'assessment.resource.json')
        self.assertEqual(response['result']['canonical_result']['document'], canonical)
        self.assertEqual(resource['document'], canonical)
        self.assertEqual(resource['reference']['digest'], 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest())
        # Object insertion order is immaterial; semantic array order is preserved.
        payload = response['result']['canonical_result']['document']
        self.assertEqual(json.dumps(payload, sort_keys=True, separators=(',', ':')),
                         json.dumps(dict(reversed(list(payload.items()))), sort_keys=True, separators=(',', ':')))


if __name__ == '__main__':
    unittest.main()
