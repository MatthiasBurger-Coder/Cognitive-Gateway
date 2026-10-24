"""Negative operator/transport paths, including atomic install and CLI dispatch."""
import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import urllib.error
import test_service as baseline
s = baseline.s


class RuntimeBoundaryTests(unittest.TestCase):
    def test_real_response_decoder_rejects_oversize_invalid_and_error_bodies(self):
        runtime=s.Runtime(timeout=1)
        for body,expected in [(b'{}',{}),(b'{"answer":1}',{'answer':1})]:
            with patch.object(s.urllib.request,'urlopen',return_value=io.BytesIO(body)):
                self.assertEqual(runtime.call('/api/version'),expected)
            with patch.object(s.urllib.request,'urlopen',return_value=io.BytesIO(body)):
                self.assertEqual(runtime.call('/api/show',{'model':'pinned'}),expected)
        for body in [b'x'*(s.MAX_BYTES+1),b'not json',b'[]',b'{"error":"private provider detail"}']:
            with patch.object(s.urllib.request,'urlopen',return_value=io.BytesIO(body)), self.assertRaises(s.ModelError):
                runtime.call('/api/version')
        for error in [urllib.error.URLError('offline'),TimeoutError(),OSError('offline')]:
            with patch.object(s.urllib.request,'urlopen',side_effect=error),self.assertRaisesRegex(s.ModelError,'runtime_unavailable'):
                runtime.call('/api/version')

    def test_identity_is_read_from_exact_runtime_snapshot_and_rejects_incomplete_metadata(self):
        runtime=s.Runtime()
        profile={'runtime_model':'snapshot'}
        responses=[{'version':'0.11.10'},{'models':[{'name':'snapshot','digest':'a'*64}]},{'details':{'quantization_level':'Q4_K_M'},'template':'pinned template'}]
        with patch.object(runtime,'call',side_effect=responses):
            identity=runtime.identity(profile)
        self.assertEqual(identity['artifact_digest'],'sha256:'+'a'*64)
        self.assertEqual(identity['template_digest'],s.digest('pinned template'))
        with patch.object(runtime,'call',side_effect=[{'version':'v'},{'models':[]}]):
            with self.assertRaisesRegex(s.ModelError,'model_missing'):runtime.identity(profile)
        with patch.object(runtime,'call',return_value={}):
            with self.assertRaisesRegex(s.ModelError,'runtime_response_invalid'):runtime.identity(profile)
        with self.assertRaisesRegex(s.ModelError,'runtime_unsupported'):runtime.check({'runtime':'unsupported'})


class OperatorTests(unittest.TestCase):
    def test_all_cli_operations_dispatch_and_fail_without_leaking_exceptions(self):
        registry=MagicMock();registry.state={'profiles':{}}
        registry.locked.return_value.__enter__.return_value=registry
        for op in ['install','test','promote','rollback','disable','inspect']:
            method='qualify' if op=='test' else op
            getattr(registry,method).return_value={'operation':op}
            with patch.object(s,'Registry',return_value=registry),patch('sys.argv',['service',op,'candidate']),contextlib.redirect_stdout(io.StringIO()):
                s.main()
        self.assertTrue(registry.qualify.called)
        with patch.object(s,'ThreadingHTTPServer') as server,patch('sys.argv',['service','serve']):
            s.main();server.return_value.serve_forever.assert_called_once()
        for error in [s.ModelError('qualification_required'),ValueError('sensitive detail')]:
            registry.qualify.side_effect=error
            output=io.StringIO()
            with patch.object(s,'Registry',return_value=registry),patch('sys.argv',['service','test','candidate']),contextlib.redirect_stdout(output),self.assertRaises(SystemExit) as exit:
                s.main()
            self.assertEqual(exit.exception.code,1)
            self.assertNotIn('sensitive detail',output.getvalue())

    def test_profile_and_install_failures_keep_registry_atomic(self):
        with tempfile.TemporaryDirectory() as temp:
            runtime=baseline.FakeRuntime();registry=s.Registry(Path(temp)/'state.json',runtime)
            source=s.load(baseline.ROOT/'models/qwen3-8b-q4.json')
            with registry.locked():
                for change,error in [({'runtime':'unsupported'},'runtime_unsupported'),({'lifecycle':'active','qualification_status':'passed'},'candidate_required'),({'artifact_digest':'sha256:'+'a'*64},'provenance_changed')]:
                    value={**source,**change};path=Path(temp)/'profile.json';path.write_text(json.dumps(value))
                    with self.assertRaisesRegex(s.ModelError,error):registry.install(path)
                path=Path(temp)/'profile.json';path.write_text(json.dumps(source));registry.install(path)
                original=registry.path.read_bytes()
                with patch.object(s.os,'replace',side_effect=OSError('disk unavailable')),self.assertRaises(OSError):
                    registry.save('failed-write','candidate')
                self.assertEqual(registry.path.read_bytes(),original)
                registry.qualify(source['model_id']);registry.promote(source['model_id'])
                with self.assertRaisesRegex(s.ModelError,'already_active'):registry.promote(source['model_id'])
                with self.assertRaisesRegex(s.ModelError,'candidate_required'):registry.qualify(source['model_id'])
                with self.assertRaises(s.ModelError):registry.rollback('missing-role')
                suite=s.load(s.SUITE)
                with self.assertRaisesRegex(s.ModelError,'contract_unsupported'):registry.infer({'role':source['role'],'output_schema':{}})
                case=suite['cases'][0]
                result=registry.infer({'role':source['role'],'output_schema':suite['output_schema'],'prompt':case['prompt']})
                self.assertEqual(result['proposal'],case['expected'])

    def test_failed_performance_memory_and_cpu_gates_retain_failure(self):
        for fault in ['latency','memory','vram','failure-probe']:
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as temp:
                runtime=baseline.FakeRuntime();registry=s.Registry(Path(temp)/'state.json',runtime)
                source=s.load(baseline.ROOT/'models/qwen3-8b-q4.json');path=Path(temp)/'profile.json';path.write_text(json.dumps(source))
                with registry.locked():
                    registry.install(path)
                    original_generate=runtime.generate;original_call=runtime.call
                    def generate(*args,**kwargs):
                        result=original_generate(*args,**kwargs)
                        if fault=='latency':result['metrics']['latency_seconds']=1000000
                        return result
                    def call(route,payload=None):
                        result=original_call(route,payload)
                        if route=='/api/ps' and fault=='memory':result['models']=[]
                        if route=='/api/ps' and fault=='vram':result['models'][0]['size_vram']=1
                        return result
                    with patch.object(runtime,'generate',side_effect=generate),patch.object(runtime,'call',side_effect=call),contextlib.ExitStack() as stack:
                        if fault=='failure-probe':stack.enter_context(patch.object(s.Runtime,'call',return_value={}))
                        with self.assertRaises(s.ModelError):registry.qualify(source['model_id'])
                    self.assertFalse(registry.state['profiles'][source['model_id']]['evidence']['passed'])
                    self.assertEqual(registry.state['profiles'][source['model_id']]['profile']['qualification_status'],'failed')
