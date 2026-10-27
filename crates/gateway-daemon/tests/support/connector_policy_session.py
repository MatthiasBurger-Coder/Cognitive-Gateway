"""Installed #230 admission matrix using real cg-mcp, PostgreSQL and stdio MCP."""
import json
import queue
import sys
import tempfile
import threading
import time
from pathlib import Path

import observed_goal_session as fixture


def run(case):
    late = case in ('late-consent', 'late-policy', 'late-inventory')
    early = case in ('policy-denied', 'missing-evidence', 'mutate-as-read', 'admin-as-read')
    with tempfile.TemporaryDirectory(prefix='cg230-installed-') as temporary:
        root = Path(temporary)
        launch, base, intent, _ = fixture.configure(root, False, policy_case=case)
        admission_path = Path(launch[13])
        if late:
            (root / 'pause-discovery').touch()

        def request(operation, inputs):
            return {'schema_version': '2.0', 'scope': base['scope'],
                    'operation': 'session.' + operation,
                    'correlation': {'request_id': 'policy-qualification'},
                    'execution': {'operating_mode': 'DEVELOPMENT', 'execution_profile': 'FULL_PATH'},
                    'input': inputs}

        client = fixture.Client(launch, environment=dict(fixture.canonical.CHILD_ENV))
        try:
            assert 'result' in client.initialize()
            started = fixture.call(client, request('start', {
                'command_id': 'start', 'intent': {'kind': 'document', 'contract': 'cg.intent',
                                                'contract_version': '1.0', 'document': intent}}), 'error' if early else 'ok')
            if early:
                done = started
            else:
                snapshot = started['result']['session']
                assert snapshot['state'] == 'runnable', started
                continuation = request('continue', {'session_id': snapshot['session'],
                                                    'command_id': 'policy-check',
                                                    'expected_revision': snapshot['revision']})
                if late:
                    responses = queue.Queue()

                    def continuing():
                        try:
                            responses.put(client.exchange('tools/call', {
                                'name': 'cg_session_continue_v2', 'arguments': continuation}))
                        except BaseException as error:
                            responses.put(error)

                    worker = threading.Thread(target=continuing)
                    worker.start()
                    until = time.monotonic() + 20
                    while not (root / 'discovery-waiting').exists():
                        assert time.monotonic() < until, 'actual discovery barrier not reached'
                        time.sleep(.01)
                    if case == 'late-inventory':
                        path = root / 'wire-tool.json'
                        tool = json.loads(path.read_text())
                        tool['inputSchema']['properties']['query']['description'] = 'changed during discovery'
                        path.write_text(json.dumps(tool))
                    else:
                        admission = json.loads(admission_path.read_text())
                        if case == 'late-consent':
                            admission['mappings'][0]['sessions']['consent_required'] = True
                        else:
                            policy = admission['mappings'][0]['canonical']['policy']
                            policy['steps'][next(iter(policy['steps']))]['authorizations']['architecture.dependency-analysis'] = 'DENIED'
                        admission_path.write_text(json.dumps(admission))
                    (root / 'release-discovery').touch()
                    worker.join(timeout=45)
                    assert not worker.is_alive(), 'installed admission failed to return'
                    response = responses.get_nowait()
                    assert isinstance(response, dict), type(response)
                    done = response['result']['structuredContent']
                else:
                    done = fixture.call(client, continuation, 'error')
            assert done['status'] == 'error', done
            assert len(done['diagnostics']) == 1, done
            assert done['diagnostics'][0]['code'] == (
                'CG_INVALID_INPUT' if case == 'late-inventory' else 'CG_POLICY_DENIED'), done
        finally:
            (root / 'release-discovery').touch()
            assert client.close() == 0

        path = root / 'wire.jsonl'
        wire = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        assert not [entry for entry in wire if entry['method'] in ('tools/call', 'resources/read')], wire
        if late or case in ('mapping-consent', 'global-consent', 'forged-consent'):
            assert any(entry['method'] == 'tools/list' for entry in wire), 'discovery must not supply permission'
        admission = json.loads(admission_path.read_text())
        scope = admission['mappings'][0]['canonical_scope']
        assert scope.replace('-', '').isalnum()
        journals = json.loads(fixture.sql(
            "SELECT coalesce(json_agg(payload::jsonb),'[]') FROM cg_cognitive_journals WHERE kind='task-sessions-v2' AND scope='" + scope + "'"))
        journal = journals[0] if journals else {}
        sessions = journal.get('sessions', [])
        if early:
            assert not sessions, 'denied admission cannot create a session'
        else:
            checkpoint = sessions[0]['checkpoint']
            assert checkpoint['snapshot']['state'] != 'completed'
            assert checkpoint.get('pre_execution_goal') is None
        assert not journal.get('grants', []), 'fixture facts and discovery cannot mint consent'
        audits = [json.loads(bytes(entry['bytes'])) for entry in journal.get('observation_policy_audits', [])]
        if case in ('mapping-consent', 'global-consent', 'forged-consent'):
            assert audits and all(entry['outcome']['kind'] == 'DENIED' for entry in audits), audits
            assert all(entry['snapshot']['consent_receipt_digest'] is None for entry in audits), audits
        print('INSTALLED_POLICY_QUALIFICATION_COMPLETE ' + case + ' ' + done['diagnostics'][0]['code'], flush=True)


if __name__ == '__main__':
    run(sys.argv[1])
