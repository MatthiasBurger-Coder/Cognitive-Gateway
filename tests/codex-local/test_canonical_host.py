"""Shipped local host uses strict canonical records and the real Rust services."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest

import test_qualification as existing

ROOT, BIN = existing.ROOT, existing.BIN
CHILD_ENV = {'LLVM_PROFILE_FILE': os.environ['LLVM_PROFILE_FILE']} if 'LLVM_PROFILE_FILE' in os.environ else {}
FIXTURE = ROOT / 'tests/fixtures/declarative-cli'


class CanonicalHost(unittest.TestCase):
    setUp = existing.Qualification.setUp
    setup_project = existing.Qualification.setup_project
    def client(self):
        client = existing.Client(self.launch, environment=CHILD_ENV)
        self.addCleanup(client.close)
        self.assertIn('result', client.initialize())
        return client

    def prepare(self, scope="external-project"):
        repository = Path(self.launch[15])
        catalog = repository / 'catalog'
        shutil.copytree(FIXTURE / 'catalog', catalog)
        def canonical(command, *options):
            result = subprocess.run([str((BIN / 'cg').resolve()), command, *options, '--json'],
                                    env=CHILD_ENV, capture_output=True, text=True, timeout=15, check=True)
            return json.loads(result.stdout)
        context = json.loads((FIXTURE / 'context.json').read_text())
        context['scope'] = scope
        assessment = canonical('assess', '--context', json.dumps(context))
        plan = canonical('plan', '--context', json.dumps(assessment), '--intent', str(FIXTURE / 'intent.json'),
                         '--catalog', str(catalog), '--rules', str(FIXTURE / 'rules.json'))
        admission_path = Path(self.launch[13])
        admission = json.loads(admission_path.read_text())
        mapping = admission['mappings'][0]
        mapping['canonical_scope'] = scope
        projection = json.loads((FIXTURE / 'projection.json').read_text())
        policy = json.loads((FIXTURE / 'policy.json').read_text())
        if scope != 'external-project':
            resolved = canonical('resolve', '--plan', json.dumps(plan), '--catalog', str(catalog),
                                 '--rules', str(FIXTURE / 'rules.json'), '--process', str(FIXTURE / 'process.json'))
            projection['basis'] = copy.deepcopy(resolved['resolution']['basis'])
            policy['basis'] = copy.deepcopy(resolved['resolution']['basis'])
        documents = [('plan', 'cg.plan', plan), ('rules', 'cg.composition-rules', json.loads((FIXTURE / 'rules.json').read_text())),
                     ('process', 'cg.process-snapshot', json.loads((FIXTURE / 'process.json').read_text())),
                     ('projection', 'cg.context-projection', projection)]
        references = {}
        for name, contract, document in documents:
            encoded = json.dumps(document, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
            ref = {'id': name, 'contract': contract, 'contract_version': '1.0', 'revision': '1',
                   'digest': 'sha256:' + hashlib.sha256(encoded.encode()).hexdigest()}
            references[name] = ref
            mapping['resources'].append({'schema_version': '1.0', 'scope': mapping['scope'], 'reference': ref,
                                         'document': document, 'provenance': [{'reference': ref, 'source_id': name,
                                         'source_revision': '1', 'freshness': 'current', 'sensitivity': 'NORMAL', 'lineage': []}]})
        mapping['canonical'] = {'catalog': str(catalog), **{name: copy.deepcopy(references[name]) for name in ['plan', 'rules', 'process']},
                                'policy': policy}
        admission_path.write_text(json.dumps(admission))
        self.admission_path, self.admission, self.references = admission_path, admission, references
        self.canonical = canonical
        self.plan = plan
        self.catalog = catalog

    def request_for(self, operation, inputs):
        request = copy.deepcopy(self.request)
        request['operation'], request['input'] = operation, inputs
        return request

    def cli_envelope(self, operation, request):
        path = self.root / (operation + '.json')
        path.write_text(json.dumps(request))
        result = subprocess.run([str((BIN / 'cg-local').resolve()), '--operation', operation,
                                 '--request', str(path), *self.launch], env=CHILD_ENV, capture_output=True,
                                text=True, timeout=15, check=True)
        return json.loads(result.stdout)

    def test_delivered_resolve_explain_compile_parity_and_exact_lineage(self):
        self.prepare()
        client = self.client()
        request = self.request_for('capabilities.resolve', {name: self.references[name] for name in ['plan', 'rules', 'process']})
        resolved = client.call(request)
        self.assertEqual(resolved['status'], 'ok', resolved)
        self.assertEqual(resolved, self.cli_envelope('capabilities.resolve', request))
        self.assertEqual(resolved, client.call(request))
        reference = next(p['reference'] for p in resolved['provenance'] if p['reference']['id'] == 'local-resolution')
        scope = self.request['scope']
        uri = f"cg://workspaces/{scope['workspace_id']}/projects/{scope['project_id']}/bindings/{scope['binding_id']}/references/{reference['id']}/{reference['revision']}/{reference['digest']}"
        resource = client.exchange('resources/read', {'uri': uri})
        self.assertEqual(json.loads(resource['result']['contents'][0]['text'])['document'], resolved['result']['canonical_result']['document'])
        self.assertIn('error', client.exchange('resources/read', {'uri': uri.replace(reference['digest'], 'sha256:' + '0' * 64)}))
        lineage = next(p['lineage'] for p in resolved['provenance'] if p['reference'] == reference)
        self.assertEqual(lineage, [self.references[name] for name in ['plan', 'rules', 'process']])
        self.assertEqual(resolved['result']['canonical_result']['document'], self.canonical('resolve',
            '--plan', json.dumps(self.plan), '--catalog', str(self.catalog), '--rules', str(FIXTURE / 'rules.json'),
            '--process', str(FIXTURE / 'process.json'))['resolution'])
        explain = self.request_for('state.explain', {'resolution': reference})
        explanation = client.call(explain)
        self.assertEqual(explanation['status'], 'ok', explanation)
        self.assertEqual(explanation, self.cli_envelope('state.explain', explain))
        self.assertEqual(explanation['result']['canonical_result']['document'], self.canonical('explain',
            '--plan', json.dumps(self.plan), '--catalog', str(self.catalog), '--rules', str(FIXTURE / 'rules.json'),
            '--process', str(FIXTURE / 'process.json'))['explanation'])
        compile_request = self.request_for('context.compile', {'resolution': reference, 'projection': self.references['projection'],
                                                              'step_id': 'step-condition.0', 'candidates': []})
        compiled = client.call(compile_request)
        self.assertEqual(compiled['status'], 'ok', compiled)
        self.assertEqual(compiled, self.cli_envelope('context.compile', compile_request))
        expected = self.canonical('compile', '--plan', json.dumps(self.plan), '--catalog', str(self.catalog),
                                  '--rules', str(FIXTURE / 'rules.json'), '--process', str(FIXTURE / 'process.json'),
                                  '--policy', str(FIXTURE / 'policy.json'), '--projection', str(FIXTURE / 'projection.json'))
        # Compare the complete canonical artifact under the documented host disclosure policy.
        expected['execution_context'] = {'id': expected['execution_context']['id'], 'representation': 'redacted'}
        if expected['user_input'] is not None:
            expected['user_input'] = {'kind': 'user_input', 'trust': 'CALLER_INPUT',
                                      'representation': 'redacted', 'content': '[REDACTED]'}
        for field in ['task', 'output_contract', 'constraints']:
            expected['gateway'][field] = {'representation': 'redacted'}
        for fragment in expected['dynamic']:
            fragment.update(content='[REDACTED]', representation='redacted',
                            provenance={'source': '[REDACTED]', 'revision': None}, evidence=[],
                            rationale='[REDACTED]', validation=None)
        self.assertEqual(compiled['result']['canonical_result']['document'], expected)
        stale = copy.deepcopy(explain)
        stale['input']['resolution']['digest'] = 'sha256:' + '0' * 64
        self.assertEqual(client.call(stale)['diagnostics'][0]['code'], 'CG_STALE_REVISION')
        stale = copy.deepcopy(request)
        stale['input']['plan']['revision'] = '2'
        self.assertEqual(client.call(stale)['diagnostics'][0]['code'], 'CG_REFERENCE_UNAVAILABLE')
        foreign = copy.deepcopy(explain)
        foreign['scope']['workspace_id'] = 'other'
        self.assertEqual(client.call(foreign)['diagnostics'][0]['code'], 'CG_SCOPE_DENIED')

    def test_current_policy_and_projection_cannot_be_replaced_by_client_claims(self):
        self.prepare()
        self.admission['mappings'][0]['canonical']['policy']['steps']['step-condition.0']['authorizations'] = {}
        self.admission_path.write_text(json.dumps(self.admission))
        client = self.client()
        resolved = client.call(self.request_for('capabilities.resolve', {n: self.references[n] for n in ['plan', 'rules', 'process']}))
        ref = next(p['reference'] for p in resolved['provenance'] if p['reference']['id'] == 'local-resolution')
        request = self.request_for('context.compile', {'resolution': ref, 'projection': self.references['projection'],
                                                      'step_id': 'step-condition.0', 'candidates': []})
        self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_CONSENT_REQUIRED')
        request['input']['step_id'] = 'unknown-step'
        self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_INVALID_INPUT')

    def test_invalid_policy_and_projection_records_fail_without_fallback(self):
        self.prepare()
        for kind, expected in [('policy-version', 'CG_INVALID_INPUT'), ('policy-description', 'CG_POLICY_DENIED'),
                               ('policy-basis', 'CG_STALE_REVISION'), ('scope', 'CG_SCOPE_DENIED')]:
            admission = copy.deepcopy(self.admission)
            mapping = admission['mappings'][0]
            if kind == 'policy-version':
                mapping['canonical']['policy']['schema_version'] = 99
            elif kind == 'policy-description':
                mapping['canonical']['policy']['policies'][0]['description'] = ''
            elif kind == 'policy-basis':
                mapping['canonical']['policy']['basis']['scope'] = 'stale'
            else:
                mapping['canonical_scope'] = 'other-project'
            self.admission_path.write_text(json.dumps(admission))
            client = self.client()
            resolve = self.request_for('capabilities.resolve', {n: self.references[n] for n in ['plan', 'rules', 'process']})
            resolved = client.call(resolve)
            if kind == 'scope':
                self.assertEqual(resolved['diagnostics'][0]['code'], expected)
            else:
                ref = next(p['reference'] for p in resolved['provenance'] if p['reference']['id'] == 'local-resolution')
                request = self.request_for('context.compile', {'resolution': ref, 'projection': self.references['projection'],
                                                              'step_id': 'step-condition.0', 'candidates': []})
                self.assertEqual(client.call(request)['diagnostics'][0]['code'], expected)
            client.close()

    def test_canonical_admission_cannot_cross_roots_or_admit_secret_sources(self):
        self.prepare()
        for kind, expected in [('outside-root', 'CG_SCOPE_DENIED'), ('missing-record', 'CG_REFERENCE_UNAVAILABLE'),
                               ('secret', 'CG_SENSITIVITY_DENIED'), ('reserved-id', 'CG_INVALID_INPUT'), ('wrong-contract', 'CG_INVALID_INPUT')]:
            admission = copy.deepcopy(self.admission)
            mapping = admission['mappings'][0]
            if kind == 'outside-root':
                mapping['canonical']['catalog'] = str(self.root)
            elif kind == 'missing-record':
                mapping['canonical']['plan']['revision'] = 'missing'
            elif kind == 'secret':
                mapping['resources'][1]['provenance'][0]['sensitivity'] = 'SECRET'
            elif kind == 'reserved-id':
                mapping['resources'][0]['reference']['id'] = 'local-resolution'
                mapping['resources'][0]['provenance'][0]['reference']['id'] = 'local-resolution'
            else:
                mapping['canonical']['plan']['contract'] = 'cg.intent'
            self.admission_path.write_text(json.dumps(admission))
            result = subprocess.run([str((BIN / 'cg-local').resolve()), '--check', *self.launch],
                                    env=CHILD_ENV, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2, (kind, result.stdout, result.stderr))
            self.assertIn(expected, result.stderr)

    def test_generated_identity_does_not_shadow_inspection_only_resources(self):
        self.prepare()
        mapping = self.admission['mappings'][0]
        del mapping['canonical']
        resource = mapping['resources'][0]
        resource['reference']['id'] = 'local-resolution'
        resource['provenance'][0]['reference']['id'] = 'local-resolution'
        self.admission_path.write_text(json.dumps(self.admission))
        client = self.client()
        scope, reference = self.request['scope'], resource['reference']
        uri = f"cg://workspaces/{scope['workspace_id']}/projects/{scope['project_id']}/bindings/{scope['binding_id']}/references/{reference['id']}/{reference['revision']}/{reference['digest']}"
        response = client.exchange('resources/read', {'uri': uri})
        self.assertNotIn('error', response, response)
        self.assertEqual(json.loads(response['result']['contents'][0]['text'])['document'], resource['document'])

    def test_catalog_drift_invalidates_resolve_explain_and_compile(self):
        self.prepare()
        client = self.client()
        resolve = self.request_for('capabilities.resolve', {n: self.references[n] for n in ['plan', 'rules', 'process']})
        result = client.call(resolve)
        self.assertEqual(result['status'], 'ok', result)
        reference = next(p['reference'] for p in result['provenance'] if p['reference']['id'] == 'local-resolution')
        skill_path = next((self.catalog / 'skills').glob('*.json'))
        skill = json.loads(skill_path.read_text())
        skill['description'] += ' changed after admission'
        skill_path.write_text(json.dumps(skill))
        for request in [resolve,
                        self.request_for('state.explain', {'resolution': reference}),
                        self.request_for('context.compile', {'resolution': reference, 'projection': self.references['projection'],
                                                            'step_id': 'step-condition.0', 'candidates': []})]:
            self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_STALE_REVISION')

    def test_identical_documents_cannot_substitute_pinned_reference_identity(self):
        self.prepare()
        mapping = self.admission['mappings'][0]
        alias = copy.deepcopy(next(r for r in mapping['resources'] if r['reference'] == self.references['plan']))
        alias['reference']['id'] = 'plan-alias'
        alias['provenance'][0]['reference'] = copy.deepcopy(alias['reference'])
        mapping['resources'].append(alias)
        self.admission_path.write_text(json.dumps(self.admission))
        client = self.client()
        request = self.request_for('capabilities.resolve', {n: self.references[n] for n in ['plan', 'rules', 'process']})
        request['input']['plan'] = alias['reference']
        self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_REFERENCE_UNAVAILABLE')
        request = self.request_for('state.explain', {'resolution': {**self.references['plan'], 'contract': 'cg.resolution'}})
        self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_REFERENCE_UNAVAILABLE')
