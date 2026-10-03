import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('model_service', ROOT / 'services/local-model/service.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)


class FakeRuntime:
    acceleration = 'cpu'

    def __init__(self):
        self.changed = False
        self.bad = False
        self.down = False

    def call(self, path, payload=None):
        if self.down:
            raise s.ModelError('runtime_unavailable')
        if path == '/api/ps':
            return {'models': [{'name': self.current, 'size': 5_000_000_000, 'size_vram': 0}]}
        return {}

    def identity(self, profile):
        if self.down:
            raise s.ModelError('runtime_unavailable')
        self.current = profile['runtime_model']
        return {'artifact_digest': s.digest(profile['model_id'] + ('changed' if self.changed else '')),
                'runtime_version': '0.11.10', 'quantization': 'Q4_K_M', 'template_digest': s.digest('template')}

    def check(self, profile):
        return s.Runtime.check(self, profile)

    def generate(self, profile, request, system, cold=False):
        if self.down:
            raise s.ModelError('runtime_unavailable')
        suite = s.load(s.SUITE)
        proposal = next(copy.deepcopy(c['expected']) for c in suite['cases'] if c['prompt'] == request['prompt'])
        if self.bad:
            proposal['target'] = 'invented.txt'
        return {'proposal': proposal, 'metrics': {'latency_seconds': 1, 'tokens_per_second': 10},
                'model_id': profile['model_id'], 'artifact_digest': profile['artifact_digest']}


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.runtime = FakeRuntime()
        self.registry = s.Registry(Path(self.temp.name) / 'state.json', self.runtime)
        self.profile = s.load(ROOT / 'models/qwen3-8b-q4.json')

    def install(self, model_id, **changes):
        profile = copy.deepcopy(self.profile)
        profile['model_id'] = model_id
        profile.update(changes)
        path = Path(self.temp.name) / (model_id + '.json')
        path.write_text(json.dumps(profile))
        return self.registry.install(path)

    def test_upgrade_rollback_and_audit_survive_restart(self):
        with self.registry.locked() as r:
            self.install('old')
            with self.assertRaisesRegex(s.ModelError, 'qualification_required'):
                r.promote('old')
            r.qualify('old')
            r.promote('old')
            self.install('replacement', model_family='compatible-fixture', model_version='next-generation')
            self.assertEqual(r.state['aliases']['semantic-interpreter'], 'old')
            r.qualify('replacement')
            r.promote('replacement')
            self.assertEqual(r.profile('old')['lifecycle'], 'deprecated')
        restarted = s.Registry(self.registry.path, self.runtime)
        with restarted.locked() as r:
            r.rollback('semantic-interpreter')
            self.assertEqual(r.active('semantic-interpreter')['model_id'], 'old')
            self.assertEqual(r.state['audit'][-1]['operation'], 'rollback')
            r.disable('replacement')
            with self.assertRaisesRegex(s.ModelError, 'qualification_required'):
                r.promote('replacement')
            with self.assertRaisesRegex(s.ModelError, 'active_disable_forbidden'):
                r.disable('old')

    def test_failed_candidate_keeps_active_and_evidence(self):
        with self.registry.locked() as r:
            self.install('old'); r.qualify('old'); r.promote('old')
            self.install('bad')
            self.runtime.bad = True
            with self.assertRaisesRegex(s.ModelError, 'semantic_gate_failed'):
                r.qualify('bad')
            self.assertEqual(r.active('semantic-interpreter')['model_id'], 'old')
            self.assertFalse(r.state['profiles']['bad']['evidence']['passed'])
            with self.assertRaisesRegex(s.ModelError, 'qualification_required'):
                r.promote('bad')

    def test_changed_provenance_and_suite_invalidate_qualification(self):
        with self.registry.locked() as r:
            self.install('old'); r.qualify('old'); r.promote('old')
            self.runtime.changed = True
            with self.assertRaisesRegex(s.ModelError, 'provenance_changed'):
                r.active('semantic-interpreter')
            self.runtime.changed = False
            with patch.object(s, 'SUITE', Path(self.temp.name) / 'suite.json'):
                suite = s.load(ROOT / 'models/suites/semantic-proposal-v1.json')
                suite['system'] += ' changed'
                s.SUITE.write_text(json.dumps(suite))
                with self.assertRaisesRegex(s.ModelError, 'qualification_required'):
                    r.active('semantic-interpreter')

    def test_snapshot_and_immutable_ids(self):
        with self.registry.locked():
            profile = self.install('old')
            self.assertTrue(profile['runtime_model'].startswith('cg-'))
            with self.assertRaisesRegex(s.ModelError, 'immutable_profile_exists'):
                self.install('old')

    def test_unavailable_and_missing_active_fail_closed(self):
        with self.registry.locked() as r:
            with self.assertRaisesRegex(s.ModelError, 'active_model_missing'):
                r.infer({'role': 'semantic-interpreter'})
            self.install('old'); r.qualify('old'); r.promote('old')
            self.runtime.down = True
            with self.assertRaisesRegex(s.ModelError, 'runtime_unavailable'):
                r.active('semantic-interpreter')

    def test_rollback_failure_preserves_history(self):
        with self.registry.locked() as r:
            self.install('old'); r.qualify('old'); r.promote('old')
            self.install('new'); r.qualify('new'); r.promote('new')
            r.disable('old')
            before = copy.deepcopy(r.state)
            with self.assertRaisesRegex(s.ModelError, 'qualification_required'):
                r.rollback('semantic-interpreter')
            self.assertEqual(before, r.state)


class TransportTests(unittest.TestCase):
    def test_unavailable_runtime(self):
        with self.assertRaisesRegex(s.ModelError, 'runtime_unavailable'):
            s.Runtime('http://127.0.0.1:1', timeout=0.1).call('/api/version')

    def test_contract_validation(self):
        with self.assertRaisesRegex(s.ModelError, 'contract_invalid'):
            s.validate({'type': 'integer'}, 'invented')
        with self.assertRaisesRegex(s.ModelError, 'contract_invalid'):
            s.validate({'type': 'invalid-type'}, {})

    def test_reference_profile_schema(self):
        s.validate(s.load(s.SCHEMA), s.load(ROOT / 'models/qwen3-8b-q4.json'))
        bad = s.load(ROOT / 'models/qwen3-8b-q4.json')
        bad['artifact_digest'] = 'mutable-tag'
        with self.assertRaises(s.ModelError):
            s.validate(s.load(s.SCHEMA), bad)

class GenerationTests(unittest.TestCase):
    def setUp(self):
        self.profile = s.load(ROOT / 'models/qwen3-8b-q4.json')
        self.suite = s.load(s.SUITE)
        self.request = {'schema_version': '1.0', 'input_contract': self.suite['input_contract'],
                        'output_contract': self.suite['output_contract'], 'prompt': 'Read README.md',
                        'output_schema': self.suite['output_schema']}
        self.response = {'done': True, 'response': json.dumps(self.suite['cases'][0]['expected']),
                         'eval_count': 20, 'eval_duration': 1_000_000_000, 'load_duration': 100}

    def test_cpu_generation_cold_unload_schema_and_metrics(self):
        runtime = s.Runtime()
        with patch.object(runtime, 'call', return_value=self.response) as call:
            result = runtime.generate(self.profile, self.request, self.suite['system'], cold=True)
            self.assertEqual(result['kind'], 'proposal')
            self.assertEqual(result['metrics']['tokens_per_second'], 20)
            self.assertEqual(call.call_args_list[0].args[1]['keep_alive'], 0)
            payload = call.call_args_list[-1].args[1]
            self.assertEqual(payload['options']['num_gpu'], 0)
            self.assertFalse(payload['stream'])
            self.assertFalse(payload['think'])
            self.assertEqual(payload['format'], self.request['output_schema'])
            self.assertEqual(json.loads(payload['prompt']), {'request': self.request['prompt']})

    def test_invalid_outputs_and_requests(self):
        runtime = s.Runtime()
        for response, code in [({'done': False}, 'output_incomplete'),
                               ({'done': True, 'response': 'bad'}, 'output_invalid'),
                               ({'done': True, 'response': '{}'}, 'contract_invalid'),
                               ({'done': True, 'done_reason': 'length'}, 'output_incomplete')]:
            with patch.object(runtime, 'call', return_value=response):
                with self.assertRaisesRegex(s.ModelError, code):
                    runtime.generate(self.profile, self.request, '')
        for field, value in [('schema_version', '2.0'), ('input_contract', 'foreign'),
                             ('output_contract', 'foreign'), ('prompt', ''), ('output_schema', None),
                             ('output_schema', {'type': 'bad'})]:
            invalid = {**self.request, field: value}
            with self.assertRaises(s.ModelError):
                runtime.generate(self.profile, invalid, '')

    def test_gpu_changes_options_only(self):
        for backend in ('gpu', 'nvidia', 'amd', 'vulkan'):
            with self.subTest(backend=backend):
                runtime = s.Runtime(acceleration=backend)
                self.assertEqual(runtime.acceleration, 'nvidia' if backend == 'gpu' else backend)
                with patch.object(runtime, 'call', return_value=self.response) as call:
                    runtime.generate(self.profile, self.request, '')
                    self.assertNotIn('num_gpu', call.call_args.args[1]['options'])
        with self.assertRaisesRegex(s.ModelError, 'configuration_invalid'):
            s.Runtime(acceleration='unknown')

    def test_qualification_binding_distinguishes_gpu_backends(self):
        bindings = {
            backend: s.Registry(runtime=s.Runtime(acceleration=backend)).binding(self.profile, self.suite)
            for backend in ('cpu', 'nvidia', 'amd', 'vulkan', 'gpu')
        }
        self.assertEqual(bindings['gpu'], bindings['nvidia'])
        self.assertEqual(len({bindings[backend] for backend in ('cpu', 'nvidia', 'amd', 'vulkan')}), 4)

class HttpBoundaryTests(unittest.TestCase):
    def test_health_and_missing_active_over_real_http(self):
        import threading
        import urllib.request
        import urllib.error
        with tempfile.TemporaryDirectory() as temporary:
            with patch.dict('os.environ', {'CG_MODEL_REGISTRY': str(Path(temporary) / 'registry.json')}):
                server = s.ThreadingHTTPServer(('127.0.0.1', 0), s.Handler)
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                endpoint = 'http://127.0.0.1:' + str(server.server_address[1])
                try:
                    with urllib.request.urlopen(endpoint + '/health') as response:
                        self.assertEqual(json.load(response)['status'], 'live')
                    for path, data, code in [('/ready', None, 'active_model_missing'),
                                             ('/v1/infer', b'{"role":"semantic-interpreter"}', 'active_model_missing'),
                                             ('/v1/infer', b'bad-json', 'request_invalid'),
                                             ('/missing', None, 'route_missing')]:
                        request = urllib.request.Request(endpoint + path, data=data)
                        with self.assertRaises(urllib.error.HTTPError) as caught:
                            urllib.request.urlopen(request)
                        with caught.exception as response:
                            self.assertEqual(json.load(response)['error'], code)
                finally:
                    server.shutdown()
                    server.server_close()
                    thread.join()


class ConcurrencyTests(unittest.TestCase):
    def test_candidate_benchmark_leaves_active_available(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = FakeRuntime()
            registry = s.Registry(Path(temporary) / 'state.json', runtime)
            profile = s.load(ROOT / 'models/qwen3-8b-q4.json')
            def install(model_id):
                profile['model_id'] = model_id
                path = Path(temporary) / (model_id + '.json')
                path.write_text(json.dumps(profile))
                registry.install(path)
            with registry.locked(operation=True) as r:
                install('active'); r.qualify('active'); r.promote('active')
                install('candidate')
                generate = runtime.generate
                calls = []
                def concurrent(profile, request, system, cold=False):
                    with s.Registry(registry.path, runtime).locked() as active:
                        calls.append(active.active('semantic-interpreter')['model_id'])
                    runtime.current = profile['runtime_model']
                    return generate(profile, request, system, cold=cold)
                with patch.object(runtime, 'generate', side_effect=concurrent):
                    r.qualify('candidate')
                self.assertEqual(calls, ['active'] * 6)
                self.assertEqual(r.state['aliases']['semantic-interpreter'], 'active')


if __name__ == '__main__':
    unittest.main()
