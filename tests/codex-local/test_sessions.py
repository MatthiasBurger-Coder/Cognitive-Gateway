"""Shared session qualification against shipped executables and real PostgreSQL."""
import concurrent.futures
import copy
import json
import hashlib
import time
import threading
import os
from pathlib import Path
import subprocess
import unittest
import uuid

import test_canonical_host as canonical
import test_qualification as existing
from jsonschema import Draft202012Validator

TRANSCRIPT_LOCK = threading.Lock()
V2 = existing.ROOT / 'schemas/codex/v2'
VALIDATORS = {name: Draft202012Validator(json.loads((V2 / (name + '.schema.json')).read_text()))
              for name in ('request', 'response', 'resource')}


@unittest.skipUnless(os.environ.get('CG_COGNITIVE_TEST_DATABASE'), 'requires disposable PostgreSQL test host')
class SharedSessions(unittest.TestCase):
    setUp = existing.Qualification.setUp
    setup_project = existing.Qualification.setup_project
    prepare = canonical.CanonicalHost.prepare
    client = canonical.CanonicalHost.client

    def configure(self, consent=False):
        self.prepare()
        mapping = self.admission['mappings'][0]
        # Distinct durable owner per case, stable across all CLI/MCP connections.
        mapping['session_id'] = 'owner-' + uuid.uuid4().hex
        self.launch[-1] = mapping['session_id']
        self.intent = json.loads((canonical.FIXTURE / 'intent.json').read_text())
        condition = self.intent['desired_state']['conditions'][0]
        condition.update(id='context-verified', subject='cg.context.projection.verified')
        self.intent['desired_state']['expression']['value'] = 'context-verified'
        store = self.root / 'session-store'
        store.write_text(os.environ['CG_COGNITIVE_TEST_DATABASE'])
        store.chmod(0o600)
        ref = lambda name: {k: self.references[name][k] for k in ('id', 'revision', 'digest')}
        self.config = {'store_file': str(store), 'intent': self.intent,
                       'basis': {'scope': 'external-project', 'plan': ref('plan'),
                                 'step': 'step-condition.0', 'projection': ref('projection'), 'sources': []},
                       'execution': {'mode': 'DEVELOPMENT', 'profile': 'FULL_PATH'},
                       'max_actions': 3, 'max_retries': 1, 'ttl_ms': 300000,
                       'consent_required': consent, 'enabled': True, 'issuers': [mapping['principal']]}
        mapping['sessions'] = self.config
        self.save()

    def save(self):
        self.admission_path.write_text(json.dumps(self.admission))

    def request_v2(self, op, inputs):
        return {'schema_version': '2.0', 'scope': copy.deepcopy(self.request['scope']), 'operation': 'session.' + op,
                'correlation': {'request_id': 'qualification'},
                'execution': {'operating_mode': 'DEVELOPMENT', 'execution_profile': 'FULL_PATH'}, 'input': inputs}

    def retain(self, request, response, operation=None):
        destination = os.environ.get('CG_SESSION_EVIDENCE_DIR')
        if destination:
            path = Path(destination) / 'transitions.jsonl'
            with TRANSCRIPT_LOCK, path.open('a') as transcript:
                transcript.write(json.dumps({'scenario': self._testMethodName, 'operation': operation or request['operation'],
                                             'request': request, 'response': response}, sort_keys=True) + '\n')

    def cli(self, request, operation=None):
        path = self.root / ('request-' + uuid.uuid4().hex + '.json')
        path.write_text(json.dumps(request))
        result = subprocess.run([str((existing.BIN / 'cg-local').resolve()), '--operation', operation or request['operation'],
                                 '--request', str(path), *self.launch], env=canonical.CHILD_ENV,
                                capture_output=True, text=True, timeout=30)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        response = json.loads(result.stdout)
        if operation != 'session.authority':
            VALIDATORS['response'].validate(response)
        self.retain(request, response, operation)
        return response

    def call(self, client, request):
        reply = client.exchange('tools/call', {'name': 'cg_' + request['operation'].replace('.', '_') + '_v2', 'arguments': request})
        self.assertIn('result', reply, reply)
        result = reply['result']
        VALIDATORS['response'].validate(result['structuredContent'])
        self.assertEqual(json.loads(result['content'][0]['text']), result['structuredContent'])
        self.retain(request, result['structuredContent'])
        return result['structuredContent']

    def start(self, command='start', client=None):
        request = self.request_v2('start', {'command_id': command, 'intent': {'kind': 'document', 'contract': 'cg.intent', 'contract_version': '1.0', 'document': self.intent}})
        result = self.cli(request) if client is None else self.call(client, request)
        self.assertEqual(result['status'], 'ok', result)
        return result

    def mutate(self, op, result, command, **fields):
        session = result['result']['session']
        return self.request_v2(op, {'session_id': session['session'], 'command_id': command,
                                   'expected_revision': session['revision'], **fields})

    def inspect(self, result):
        return self.request_v2('inspect', {'session_id': result['result']['session']['session']})

    def operator(self, result, decision):
        return self.cli({'session_id': result['result']['session']['session'],
                         'issuer': self.config['issuers'][0], 'decision': decision}, 'session.authority')

    def test_baseline_cli_mcp_restart_verified_evidence_and_replay(self):
        self.configure()
        client = self.client()
        tools = client.exchange('tools/list')['result']['tools']
        self.assertEqual(len(tools), 19)
        self.assertNotIn('cg_session_authority_v2', [t['name'] for t in tools])
        started = self.start(client=client)
        self.assertEqual(started['result']['session']['state'], 'runnable')
        self.assertEqual(client.close(), 0)
        continued = self.cli(self.mutate('continue', started, 'compile'))
        self.assertEqual(continued['status'], 'ok', continued)
        self.assertEqual(continued['result']['session']['state'], 'completed', continued)
        self.assertEqual(continued['result']['budget']['actions'], 1)
        reconnected = self.client()
        inspected = self.call(reconnected, self.inspect(continued))
        self.assertEqual(inspected, self.cli(self.inspect(continued)))
        reference = inspected['result']['session']['final_evidence']
        scope = self.request['scope']
        uri = f"cg://workspaces/{scope['workspace_id']}/projects/{scope['project_id']}/bindings/{scope['binding_id']}/references/{reference['id']}/{reference['revision']}/{reference['digest']}"
        resource = reconnected.exchange('resources/read', {'uri': uri})
        self.assertIn('result', resource, resource)
        resource_value = json.loads(resource['result']['contents'][0]['text'])
        VALIDATORS['resource'].validate(resource_value)
        if os.environ.get('CG_SESSION_EVIDENCE_DIR'):
            (Path(os.environ['CG_SESSION_EVIDENCE_DIR']) / 'verified-evidence.resource.json').write_text(json.dumps(resource_value, indent=2) + '\n')
        document = resource_value['document']
        self.assertEqual(document['goal_outcome'], 'SATISFIED')
        self.assertTrue(document['facts'])
        self.assertTrue(document['evidence'])
        self.assertEqual(document['verification']['basis']['projection'], self.config['basis']['projection'])
        self.assertEqual(self.start_response('start')['diagnostics'][0]['code'], 'CG_DUPLICATE_COMMAND')
        outcome = self.cli(self.request_v2('inspect', {'command_id': 'compile'}))
        self.assertEqual(outcome['result']['session']['command_outcome']['command'], 'compile')
        self.assertEqual(outcome['result']['session']['state'], 'completed')

    def start_response(self, command):
        return self.cli(self.request_v2('start', {'command_id': command, 'intent': {'kind': 'document', 'contract': 'cg.intent', 'contract_version': '1.0', 'document': self.intent}}))

    def test_persisted_consent_restart_exact_reference_and_single_use(self):
        self.configure(consent=True)
        started = self.start()
        pending = self.cli(self.mutate('continue', started, 'pause'))
        self.assertEqual(pending['status'], 'ok', pending)
        self.assertEqual(pending['result']['session']['state'], 'pending_consent')
        self.assertEqual(pending['result']['budget']['actions'], 0)
        issued = self.operator(pending, 'approve')
        self.assertEqual(issued['status'], 'ok', issued)
        reference = issued['result']['reference']
        pending_id = pending['result']['session']['pending']['id']
        approve = self.mutate('approve', pending, 'approve', pending_id=pending_id,
                              consent={'contract': 'cg.consent-record', 'contract_version': '2.0', 'reference': reference})
        wrong = copy.deepcopy(approve)
        wrong['input']['consent']['reference']['digest'] = 'sha256:' + '0' * 64
        self.assertEqual(self.cli(wrong)['status'], 'error')
        accepted = self.cli(approve)
        self.assertEqual(accepted['status'], 'ok', accepted)
        self.assertEqual(accepted['result']['session']['state'], 'runnable')
        self.assertEqual(self.cli(approve)['diagnostics'][0]['code'], 'CG_DUPLICATE_COMMAND')
        done = self.call(self.client(), self.mutate('continue', accepted, 'run'))
        self.assertEqual(done['result']['session']['state'], 'completed', done)
        self.assertEqual(self.operator(done, 'withdraw')['status'], 'ok')
        self.assertEqual(self.cli(self.inspect(done))['result'], done['result'])

    def test_competing_processes_cancel_eof_scope_and_budget(self):
        self.configure()
        started = self.start()
        requests = [self.mutate('continue', started, 'runner-' + str(i)) for i in range(2)]
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            outcomes = list(pool.map(self.cli, requests))
        self.assertEqual(sum(r['status'] == 'ok' for r in outcomes), 1, outcomes)
        inspected = self.cli(self.inspect(started))
        self.assertEqual(inspected['result']['budget']['actions'], 1)
        second = self.start('second')
        client = self.client()
        self.call(client, self.inspect(second))
        client.close()
        self.assertEqual(self.cli(self.inspect(second))['result']['session']['state'], 'runnable')
        cancelled = self.cli(self.mutate('cancel', second, 'stop'))
        self.assertEqual(cancelled['result']['session']['state'], 'cancelled')
        self.assertEqual(cancelled['result']['budget']['actions'], 0)
        foreign = self.inspect(second)
        foreign['scope']['workspace_id'] = 'foreign'
        self.assertEqual(self.cli(foreign)['diagnostics'][0]['code'], 'CG_SCOPE_DENIED')
        self.config['max_actions'] = 0
        self.save()
        self.assertEqual(self.start_response('limited')['diagnostics'][0]['code'], 'CG_LIMIT_EXCEEDED')

    def test_denial_withdrawal_current_policy_and_unrelated_goal(self):
        self.configure(consent=True)
        for decision in ('deny', 'withdraw'):
            pending = self.cli(self.mutate('continue', self.start(decision), decision + '-pause'))
            if decision == 'withdraw':
                self.assertEqual(self.operator(pending, 'approve')['status'], 'ok')
            issued = self.operator(pending, decision)
            self.assertEqual(issued['status'], 'ok', issued)
            self.assertEqual(self.cli(self.inspect(pending))['result']['session']['state'], 'failed')
        started = self.start('policy')
        self.admission['mappings'][0]['canonical']['policy']['steps']['step-condition.0']['authorizations'] = {}
        self.save()
        self.assertEqual(self.cli(self.mutate('continue', started, 'policy-denied'))['status'], 'error')
        unrelated = copy.deepcopy(self.intent)
        unrelated['desired_state']['conditions'][0]['subject'] = 'architecture.clean'
        request = self.request_v2('start', {'command_id': 'unrelated', 'intent': {'kind': 'document', 'contract': 'cg.intent', 'contract_version': '1.0', 'document': unrelated}})
        self.assertEqual(self.cli(request)['diagnostics'][0]['code'], 'CG_UNSUPPORTED_CAPABILITY')

    def add_sources(self):
        for name in ('source-a', 'source-b'):
            document = {'id': name, 'kind': 'knowledge', 'content': 'An admitted source for the task',
                        'scope': 'external-project', 'step': 'step-condition.0', 'source': 'fixture://' + name,
                        'revision': '1', 'quality': {'trust': 'RETRIEVED_CONTENT', 'sensitivity': 'PUBLIC',
                        'confidence': {'kind': 'UNKNOWN'}, 'conflict': 'NONE', 'freshness': 'FRESH', 'uncertainty': 'NONE'},
                        'rationale': 'Task source alternative', 'evidence': [], 'validation': None}
            digest = 'sha256:' + hashlib.sha256(json.dumps(document, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
            reference = {'id': name, 'revision': '1', 'digest': digest}
            wire = {**reference, 'contract': 'cg.context-fragment', 'contract_version': '1.0'}
            self.config['basis']['sources'].append(reference)
            self.admission['mappings'][0]['resources'].append({'schema_version': '1.0', 'scope': self.request['scope'],
                'reference': wire, 'document': document, 'provenance': [{'reference': wire, 'source_id': name,
                'source_revision': '1', 'freshness': 'current', 'sensitivity': 'PUBLIC', 'lineage': []}]})
        self.save()

    def test_clarification_is_a_real_input_choice_and_never_consent(self):
        self.configure(consent=True)
        self.add_sources()
        started = self.start()
        self.assertEqual(started['result']['session']['state'], 'pending_clarification')
        self.assertEqual(started['result'], self.cli(self.inspect(started))['result'])
        source = self.config['basis']['sources'][1]
        clarify = self.mutate('clarify', started, 'answer', pending_id=started['result']['session']['pending']['id'],
                             answer={'contract': 'cg.clarification-answer', 'contract_version': '2.0', 'selected_source': source})
        wrong = copy.deepcopy(clarify)
        wrong['input']['pending_id'] = 'wrong'
        self.assertEqual(self.cli(wrong)['diagnostics'][0]['code'], 'CG_INVALID_INTERACTION')
        wrong = copy.deepcopy(clarify)
        wrong['input']['answer']['selected_source']['digest'] = 'sha256:' + '0' * 64
        self.assertEqual(self.cli(wrong)['diagnostics'][0]['code'], 'CG_INVALID_INPUT')
        answered = self.call(self.client(), clarify)
        self.assertEqual(answered['result']['session']['state'], 'runnable', answered)
        self.assertEqual(self.cli(clarify)['diagnostics'][0]['code'], 'CG_DUPLICATE_COMMAND')
        pending = self.cli(self.mutate('continue', answered, 'after-answer'))
        self.assertEqual(pending['result']['session']['state'], 'pending_consent')
        self.assertEqual(pending['result']['budget']['actions'], 0)
        self.assertEqual(self.cli(clarify)['diagnostics'][0]['code'], 'CG_DUPLICATE_COMMAND')
        issued = self.operator(pending, 'approve')['result']['reference']
        accepted = self.cli(self.mutate('approve', pending, 'permission', pending_id=pending['result']['session']['pending']['id'],
                        consent={'contract': 'cg.consent-record', 'contract_version': '2.0', 'reference': issued}))
        done = self.cli(self.mutate('continue', accepted, 'selected-compile'))
        self.assertEqual(done['result']['session']['state'], 'completed', done)

    def test_changed_action_invalidates_consent_and_expired_reply_cannot_resume(self):
        self.configure(consent=True)
        started = self.start()
        pending = self.cli(self.mutate('continue', started, 'pause'))
        issued = self.operator(pending, 'approve')['result']['reference']
        approve = self.mutate('approve', pending, 'permission', pending_id=pending['result']['session']['pending']['id'],
                             consent={'contract': 'cg.consent-record', 'contract_version': '2.0', 'reference': issued})
        self.config['consent_required'] = False
        self.save()
        self.assertEqual(self.cli(approve)['diagnostics'][0]['code'], 'CG_INVALID_INTERACTION')
        self.assertEqual(self.cli(self.inspect(pending))['result']['session']['revision'], pending['result']['session']['revision'])
        self.config['consent_required'] = True
        self.config['ttl_ms'] = 2000
        self.save()
        started = self.start('expiry')
        pending = self.cli(self.mutate('continue', started, 'expiry-pause'))
        issued = self.operator(pending, 'approve')['result']['reference']
        time.sleep(2.1)
        expired = self.cli(self.mutate('approve', pending, 'expired', pending_id=pending['result']['session']['pending']['id'],
                       consent={'contract': 'cg.consent-record', 'contract_version': '2.0', 'reference': issued}))
        self.assertEqual(expired['diagnostics'][0]['code'], 'CG_EXPIRED_INTERACTION')
        self.assertEqual(self.cli(self.inspect(pending))['result']['session']['state'], 'pending_consent')
        self.assertEqual(self.operator(pending, 'recover')['result']['session']['state'], 'failed')

    def sql(self, statement):
        result = subprocess.run(['docker', 'exec', '-i', os.environ['CG_COGNITIVE_TEST_CONTAINER'],
                                'psql', '-X', '-At', '-v', 'ON_ERROR_STOP=1', '-U', 'cognitive_gateway', '-d', 'cognitive_gateway'],
                                input=statement, text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_postgres_failure_after_artifact_commit_recovery_fences_without_redispatch(self):
        self.configure()
        started = self.start()
        session = started['result']['session']['session']
        self.sql("""CREATE FUNCTION cg_session_test_fail() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN IF NEW.kind='task-sessions-v2' AND EXISTS(SELECT 1 FROM jsonb_array_elements(NEW.payload::jsonb->'sessions') s
                WHERE s->'checkpoint'->'snapshot'->>'session'='%s' AND s->'checkpoint'->'snapshot'->>'state'='completed')
            THEN RAISE EXCEPTION 'qualification completion commit failure'; END IF; RETURN NEW; END $$;
            CREATE TRIGGER cg_session_test_fail BEFORE UPDATE ON cg_cognitive_journals FOR EACH ROW EXECUTE FUNCTION cg_session_test_fail();""" % session)
        try:
            failed = self.cli(self.mutate('continue', started, 'crash-after-artifact'))
            self.assertEqual(failed['diagnostics'][0]['code'], 'CG_STORAGE_UNAVAILABLE')
        finally:
            self.sql('DROP TRIGGER cg_session_test_fail ON cg_cognitive_journals; DROP FUNCTION cg_session_test_fail();')
        pending = self.cli(self.inspect(started))
        self.assertEqual(pending['result']['session']['state'], 'dispatching')
        self.assertEqual(pending['result']['budget']['actions'], 1)
        # Inspect remains pure and a live lease prevents takeover.
        self.assertEqual(self.operator(pending, 'recover')['diagnostics'][0]['code'], 'CG_INVALID_SESSION_STATE')
        self.assertEqual(pending, self.cli(self.inspect(started)))
        time.sleep(30.1)
        recovered = self.operator(pending, 'recover')
        self.assertEqual(recovered['status'], 'ok', recovered)
        self.assertEqual(recovered['result']['session']['state'], 'completed')
        final = self.cli(self.inspect(started))
        self.assertEqual(final['result']['budget'], pending['result']['budget'])
        self.assertEqual(self.cli(self.mutate('continue', pending, 'old-host'))['diagnostics'][0]['code'], 'CG_STALE_REVISION')

    def test_v2_discovery_strict_wire_and_current_host_denials(self):
        self.configure()
        client = self.client()
        listed = client.exchange('resources/list')['result']['resources']
        self.assertEqual(sum(r['uri'].startswith('cg://contracts/2.0/') for r in listed), 6)
        for name in ('catalog', 'common.schema.json', 'request.schema.json', 'response.schema.json', 'resource.schema.json', 'catalog.schema.json'):
            reply = client.exchange('resources/read', {'uri': 'cg://contracts/2.0/' + name})
            self.assertIn('result', reply)
            self.assertEqual(json.loads(reply['result']['contents'][0]['text']), json.loads((V2 / ('catalog.json' if name == 'catalog' else name)).read_text()))
        self.assertIn('error', client.exchange('resources/read', {'uri': 'cg://contracts/2.0/unknown'}))
        started = self.start()
        invalid = self.inspect(started)
        invalid['input']['unknown'] = True
        self.assertEqual(self.call(client, invalid)['diagnostics'][0]['code'], 'CG_INVALID_INPUT')
        invalid = self.inspect(started)
        invalid['schema_version'] = '1.0'
        self.assertEqual(self.call(client, invalid)['diagnostics'][0]['code'], 'CG_UNSUPPORTED_VERSION')
        # A frozen v1 tool name cannot invoke the new version's real mutations.
        reply = client.exchange('tools/call', {'name': 'cg_session_cancel_v1', 'arguments': self.mutate('cancel', started, 'alias')})
        self.assertEqual(reply['result']['structuredContent']['diagnostics'][0]['code'], 'CG_UNSUPPORTED_VERSION')
        wrong = self.inspect(started)
        wrong['execution']['execution_profile'] = 'NORMAL_PATH'
        self.assertEqual(self.cli(wrong)['diagnostics'][0]['code'], 'CG_INVALID_INPUT')
        wrong = self.mutate('continue', started, 'wrong-profile')
        wrong['execution']['execution_profile'] = 'NORMAL_PATH'
        self.assertEqual(self.cli(wrong)['diagnostics'][0]['code'], 'CG_INVALID_INPUT')
        self.config['issuers'] = []
        self.save()
        self.assertEqual(self.cli({'session_id': started['result']['session']['session'], 'issuer': self.admission['mappings'][0]['principal'], 'decision': 'approve'}, 'session.authority')['diagnostics'][0]['code'], 'CG_POLICY_DENIED')
        self.assertEqual(self.cli({'session_id': started['result']['session']['session'], 'issuer': 'foreign', 'decision': 'approve'}, 'session.authority')['diagnostics'][0]['code'], 'CG_POLICY_DENIED')
        self.config['enabled'] = False
        self.save()
        self.assertEqual(self.call(client, self.inspect(started))['diagnostics'][0]['code'], 'CG_POLICY_DENIED')
        self.assertEqual(len(self.client().exchange('tools/list')['result']['tools']), 13)
        self.assertEqual(self.cli(self.inspect(started))['diagnostics'][0]['code'], 'CG_UNSUPPORTED_CAPABILITY')
        self.config['enabled'] = True
        self.config['ttl_ms'] = 0
        self.save()
        self.assertEqual(self.start_response('zero-ttl')['diagnostics'][0]['code'], 'CG_LIMIT_EXCEEDED')
        self.config['ttl_ms'] = 300000
        self.config['basis']['step'] = 'changed-step'
        self.save()
        self.assertEqual(self.cli(self.mutate('continue', started, 'changed-basis'))['diagnostics'][0]['code'], 'CG_STALE_REVISION')
        self.config['basis']['step'] = 'step-condition.0'
        original_plan = self.config['basis']['plan']['digest']
        self.config['basis']['plan']['digest'] = 'sha256:' + '0' * 64
        self.save()
        self.assertEqual(self.start_response('wrong-plan')['diagnostics'][0]['code'], 'CG_STALE_REVISION')
        self.config['basis']['plan']['digest'] = original_plan
        self.config['store_file'] = 'relative'
        self.save()
        self.assertEqual(self.cli(self.inspect(started))['diagnostics'][0]['code'], 'CG_INVALID_INPUT')
        self.config['store_file'] = str(self.root / 'missing-store')
        self.save()
        self.assertEqual(self.cli(self.inspect(started))['diagnostics'][0]['code'], 'CG_SESSION_UNAVAILABLE')


if __name__ == '__main__':
    unittest.main()
