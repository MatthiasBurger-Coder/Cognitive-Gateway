import copy
import importlib.util
import sys
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'services/local-model'))
from canonical import invoke as canonical_invoke
from service import ModelError

def invoke(registry, request):
    return canonical_invoke(registry, request, ModelError)


class Runtime:
    acceleration = 'cpu'

    def __init__(self):
        self.result = {'model': 'snapshot', 'done': True, 'done_reason': 'stop',
                       'message': {'content': '"proposal"'}, 'prompt_eval_count': 10, 'eval_count': 2}
        self.checked = False

    def call(self, path, body):
        self.path, self.body = path, body
        return self.result

    def check(self, profile):
        self.checked = True


class Registry:
    def __init__(self):
        self.runtime = Runtime()
        self.data = {'model_id': 'model', 'model_version': '1', 'artifact_digest': 'sha256:' + 'a'*64,
                     'runtime_model': 'snapshot', 'lifecycle': 'active', 'context_limit': 4096,
                     'supported_input_contracts': ['prompt-ir-v1'], 'supported_output_contracts': ['result']}
        self.qualified_calls = 0

    def profile(self, model):
        if model != self.data['model_id']:
            raise ModelError('profile_missing')
        return self.data

    def qualified(self, profile):
        self.qualified_calls += 1


def request():
    return {'schema_version': '1.0', 'model_id': 'model', 'model_version': '1',
            'artifact_digest': 'sha256:' + 'a'*64, 'input_contract': 'prompt-ir-v1', 'output_contract': 'result',
            'messages': [{'role': 'system', 'content': 'boundary'}, {'role': 'user', 'content': 'data'}],
            'max_output_tokens': 128, 'context_limit': 4096, 'structured_output_mode': 'JSON_SCHEMA',
            'output_schema': {'type': 'string'},
            'options': {'temperature': 0, 'top_p': .9, 'top_k': 20, 'stop': ['END'], 'seed': 42}}


class CanonicalTests(unittest.TestCase):
    def test_actual_wire_controls_and_provenance(self):
        for mode in ['JSON', 'JSON_SCHEMA']:
            for acceleration in ['cpu', 'nvidia']:
                registry = Registry()
                registry.runtime.acceleration = acceleration
                req = request()
                req['structured_output_mode'] = mode
                result = invoke(registry, req)
                self.assertEqual(result['content'], '"proposal"')
                self.assertEqual(result['usage']['completion_tokens'], 2)
                self.assertTrue(registry.runtime.checked)
                self.assertEqual(registry.qualified_calls, 1)
                self.assertEqual(registry.runtime.path, '/api/chat')
                body = registry.runtime.body
                self.assertEqual(body['messages'], req['messages'])
                self.assertEqual(body['options']['seed'], 42)
                self.assertEqual(body['options']['num_predict'], 128)
                self.assertEqual(body['format'], 'json' if mode == 'JSON' else req['output_schema'])
                self.assertEqual('num_gpu' in body['options'], acceleration == 'cpu')
                self.assertEqual(req, {**request(), 'structured_output_mode': mode})
        registry.runtime.result.pop('eval_count')
        self.assertNotIn('usage', invoke(registry, req))

    def test_standalone_entrypoint_error_identity_is_preserved(self):
        class EntrypointError(Exception):
            pass
        req = request()
        req['schema_version'] = '2.0'
        with self.assertRaisesRegex(EntrypointError, 'request_invalid'):
            canonical_invoke(Registry(), req, EntrypointError)

    def test_readiness_does_not_add_contract_support(self):
        registry = Registry()
        registry.data['supported_input_contracts'] = ['local-inference/1.0']
        with self.assertRaisesRegex(ModelError, 'contract_unsupported'):
            invoke(registry, request())
        self.assertFalse(hasattr(registry.runtime, 'body'))

    def test_invalid_requests_and_runtime_responses(self):
        cases = [('schema_version', '2'), ('model_id', 'alias'), ('artifact_digest', 'changed'),
                 ('model_version', '2'), ('input_contract', 'unsupported'), ('output_contract', 'unknown'),
                 ('messages', None), ('messages', [{}]), ('messages', ['system', 'user']),
                 ('messages', [{'role': 'system', 'content': ''}, {'role': 'user', 'content': 'x'}]),
                 ('max_output_tokens', True), ('max_output_tokens', 0), ('max_output_tokens', 5000),
                 ('context_limit', 99999), ('options', None), ('options', {'unknown': 1}),
                 ('options', {'temperature': 3}), ('options', {'top_p': 'x'}), ('options', {'top_k': 0}),
                 ('options', {'seed': False}), ('options', {'stop': ['']}), ('structured_output_mode', 'TOOLS'),
                 ('output_schema', None)]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                req = request()
                req[key] = value
                with self.assertRaises(ModelError):
                    invoke(Registry(), req)
        registry = Registry()
        registry.data['lifecycle'] = 'candidate'
        with self.assertRaisesRegex(ModelError, 'active_model_missing'):
            invoke(registry, request())
        for key, value in [('model', 'alias'), ('done', False), ('done_reason', 'length'),
                           ('message', {}), ('message', None), ('message', {'content': 'x', 'tool_calls': ['call']}),
                           ('prompt_eval_count', -1), ('eval_count', True)]:
            registry = Registry()
            registry.runtime.result[key] = value
            with self.subTest(key=key), self.assertRaises(ModelError):
                invoke(registry, request())

    def test_http_route_is_shipped(self):
        import service
        from http.server import ThreadingHTTPServer
        from threading import Thread
        from unittest.mock import patch
        import contextlib
        import json
        import urllib.request

        @contextlib.contextmanager
        def locked(registry):
            yield registry
        with patch.object(service.Registry, 'locked', locked), patch('canonical.invoke', return_value={'kind': 'proposal'}) as bridge:
            server = ThreadingHTTPServer(('127.0.0.1', 0), service.Handler)
            thread = Thread(target=server.serve_forever)
            thread.start()
            try:
                req = urllib.request.Request(f'http://127.0.0.1:{server.server_port}/v1/invoke',
                                             data=json.dumps(request()).encode())
                with urllib.request.urlopen(req) as response:
                    self.assertEqual(json.load(response), {'kind': 'proposal'})
                bridge.assert_called_once()
                self.assertIs(bridge.call_args.args[2], service.ModelError)
            finally:
                server.shutdown()
                server.server_close()
                thread.join()
