"""EPIC-04.10 real executable qualification; synthetic client, private stdio."""
import copy
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get('CG_QUALIFICATION_BIN_DIR', ROOT / 'target/debug'))
GOLDEN = ROOT / 'tests/fixtures/codex-qualification'


class Client:
    def __init__(self, launch, extra=(), environment=None):
        self.process = subprocess.Popen([str((BIN / 'cg-mcp').resolve()), *launch, *extra],
                                        env={} if environment is None else environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True)
        self.replies = queue.Queue()
        self.errors = []
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()
        self.error_reader = threading.Thread(target=self.read_errors, daemon=True)
        self.error_reader.start()
        self.next_id = 0

    def read(self):
        for line in self.process.stdout:
            self.replies.put(line)
        self.replies.put(None)

    def read_errors(self):
        self.errors.extend(self.process.stderr)

    def exchange(self, method, params=None, raw=None, notify=False):
        self.next_id += 1
        frame = {'jsonrpc': '2.0', 'method': method}
        if not notify:
            frame['id'] = self.next_id
        if params is not None:
            frame['params'] = params
        self.process.stdin.write((raw if raw is not None else json.dumps(frame)) + '\n')
        self.process.stdin.flush()
        if notify:
            return None
        line = self.replies.get(timeout=10)
        if line is None:
            raise AssertionError('Adapter closed before response')
        reply = json.loads(line)
        if raw is None and reply.get('id') not in (frame['id'], None):
            raise AssertionError('JSON-RPC response ID differs')
        return reply

    def initialize(self, version='2025-11-25'):
        reply = self.exchange('initialize', {'protocolVersion': version, 'capabilities': {},
                                            'clientInfo': {'name': 'codex', 'version': '1.0'}})
        if 'result' in reply:
            self.exchange('notifications/initialized', notify=True)
        return reply

    def call(self, request):
        reply = self.exchange('tools/call', {'name': 'cg_' + request['operation'].replace('.', '_') + '_v1',
                                            'arguments': request})
        result = reply['result']
        if json.loads(result['content'][0]['text']) != result['structuredContent']:
            raise AssertionError('Text and structured projections differ')
        if result['isError'] != (result['structuredContent']['status'] != 'ok'):
            raise AssertionError('Error projection differs')
        return result['structuredContent']

    def close(self):
        if not self.process.stdin.closed:
            self.process.stdin.close()
        try:
            code = self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=10)
            raise AssertionError('Adapter failed to exit within bound')
        finally:
            self.reader.join(timeout=10)
            self.error_reader.join(timeout=10)
            self.process.stdout.close()
            self.process.stderr.close()
        return code


class Qualification(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cg-qualification-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.launch, self.request = self.setup_project('a')

    def setup_project(self, name):
        repository = self.root / name
        repository.mkdir()
        setup = self.root / ('setup-' + name)
        subprocess.run(['python3', str(ROOT / 'scripts/bootstrap-codex-local.py'),
                        '--repository', str(repository), '--output', str(setup),
                        '--bin-dir', str(BIN), '--client-name', 'codex', '--client-version', '1.0'],
                       check=True, capture_output=True, timeout=15)
        return json.loads((setup / 'launch.json').read_text()), json.loads((setup / 'request.json').read_text())

    def client(self, launch=None, extra=()):
        client = Client(self.launch if launch is None else launch, extra)
        self.addCleanup(client.close)
        self.assertIn('result', client.initialize())
        return client

    def test_golden_no_key_inspection_determinism_and_cli_parity(self):
        client = self.client()
        tools = client.exchange('tools/list')['result']['tools']
        self.assertEqual(len(tools), 13)
        expected = json.loads((GOLDEN / 'inspect.response.json').read_text())
        first = client.call(self.request)
        self.assertEqual(first, expected)
        # Different JSON member ordering and whitespace are equivalent inputs.
        reordered = json.loads(json.dumps(self.request, sort_keys=True, indent=3))
        self.assertEqual(client.call(reordered), expected)
        self.assertEqual(self.client().call(reordered), expected)
        request_file = self.root / 'request.json'
        request_file.write_text(json.dumps(reordered))
        cli = subprocess.run([str((BIN / 'cg-local').resolve()), '--operation', 'situation.inspect',
                              '--request', str(request_file), *self.launch], env={},
                             capture_output=True, text=True, timeout=10, check=True)
        self.assertEqual(json.loads(cli.stdout), expected)
        health = client.exchange('resources/read', {'uri': 'cg://runtime/health'})
        self.assertNotIn(str(self.root), json.dumps(health))

    def test_cross_workspace_scope_and_resource_isolation(self):
        client = self.client()
        foreign = copy.deepcopy(self.request)
        foreign['scope']['workspace_id'] = 'foreign-workspace'
        self.assertEqual(client.call(foreign)['diagnostics'][0]['code'], 'CG_SCOPE_DENIED')
        # A second real root with an overlapping resource ID has a separate binding.
        launch, request = self.setup_project('b')
        admission_path = Path(launch[13])
        admission = json.loads(admission_path.read_text())
        admission['mappings'][0]['scope']['workspace_id'] = 'workspace-b'
        for resource in admission['mappings'][0]['resources']:
            resource['scope']['workspace_id'] = 'workspace-b'
        admission_path.write_text(json.dumps(admission))
        launch[7] = 'workspace-b'
        request['scope']['workspace_id'] = 'workspace-b'
        other = self.client(launch)
        self.assertEqual(other.call(self.request)['diagnostics'][0]['code'], 'CG_SCOPE_DENIED')
        self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_SCOPE_DENIED')
        self.assertEqual(other.call(request)['scope']['workspace_id'], 'workspace-b')
        stale = copy.deepcopy(self.request)
        stale['input']['situation']['reference']['digest'] = 'sha256:' + '0' * 64
        self.assertEqual(client.call(stale)['diagnostics'][0]['code'], 'CG_REFERENCE_UNAVAILABLE')

    def test_policy_mutation_sensitivity_and_credentials_fail_closed(self):
        client = self.client()
        denied = copy.deepcopy(self.request)
        denied['execution']['execution_profile'] = 'FAST_PATH'
        self.assertEqual(client.call(denied)['diagnostics'][0]['code'], 'CG_POLICY_DENIED')
        denied = copy.deepcopy(self.request)
        denied['input']['situation'] = {'kind': 'document', 'contract': 'cg.situation',
                                       'contract_version': '1.0', 'document': {}}
        self.assertEqual(client.call(denied)['diagnostics'][0]['code'], 'CG_SENSITIVITY_DENIED')
        for operation in ('session.start', 'session.approve', 'session.cancel',
                          'session.clarify', 'session.continue'):
            request = json.loads((ROOT / 'tests/fixtures/codex-v1' / (operation + '.request.json')).read_text())
            self.assertEqual(client.call(request)['diagnostics'][0]['code'], 'CG_UNSUPPORTED_CAPABILITY')
        reply = client.exchange('tools/call', {'name': 'cg_situation_inspect_v1',
                                             'arguments': {'api_key': 'QUALIFICATION_SECRET_SENTINEL'}})
        self.assertIn('error', reply)
        self.assertNotIn('QUALIFICATION_SECRET_SENTINEL', json.dumps(reply))
        self.assertEqual(client.close(), 0)
        self.assertNotIn('QUALIFICATION_SECRET_SENTINEL', ''.join(client.errors))
        output = subprocess.run([str((BIN / 'cg-mcp').resolve()), *self.launch],
                                env={'OPENAI_API_KEY': 'QUALIFICATION_SECRET_SENTINEL'},
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(output.returncode, 2)
        self.assertEqual(output.stdout, '')
        self.assertNotIn('QUALIFICATION_SECRET_SENTINEL', output.stderr)

    def test_protocol_schema_malformed_and_duplicate_requests(self):
        wrong = Client(self.launch)
        self.addCleanup(wrong.close)
        self.assertIn('error', wrong.initialize('2099-01-01'))
        client = self.client()
        invalid = copy.deepcopy(self.request)
        invalid['schema_version'] = '9.0'
        self.assertEqual(client.call(invalid)['diagnostics'][0]['code'], 'CG_UNSUPPORTED_VERSION')
        for raw in ('{', '{"jsonrpc":"2.0","id":8,"id":9,"method":"ping"}'):
            self.assertIn('error', client.exchange('ping', raw=raw))
        self.assertIn('result', client.exchange('ping'))

    def test_disconnect_reconnect_and_idle_deadline(self):
        client = self.client()
        self.assertEqual(client.call(self.request)['status'], 'ok')
        self.assertEqual(client.close(), 0)
        self.assertEqual(self.client().call(self.request)['status'], 'ok')
        limits = self.root / 'limits.json'
        limits.write_text(json.dumps({'input_bytes': 1048576, 'output_bytes': 1048576,
                                     'requests': 10000, 'request_timeout_ms': 30000,
                                     'idle_timeout_ms': 20, 'write_timeout_ms': 2000}))
        idle = Client(self.launch, ('--runtime-limits', str(limits)))
        self.addCleanup(idle.close)
        self.assertNotEqual(idle.process.wait(timeout=10), 0)
        idle.close()
        self.assertIn('CG_TRANSPORT_TIMEOUT', ''.join(idle.errors))


if __name__ == '__main__':
    unittest.main()
