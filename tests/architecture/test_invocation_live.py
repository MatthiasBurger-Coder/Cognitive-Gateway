"""Deterministic grader checks; these tests never label a mocked model as live evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
SPEC=importlib.util.spec_from_file_location('invocation_live',ROOT/'tests/scripts/qualify-invocation-live.py')
LIVE=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LIVE)


class LiveQualificationTests(unittest.TestCase):
    def test_prompt_revision_preserves_original_cases_and_adds_control_cases(self):
        original=json.loads((ROOT/'models/suites/semantic-proposal-v1.json').read_text())
        candidate=json.loads((ROOT/'models/suites/semantic-proposal-v3.json').read_text())
        indexed={c['id']:c for c in candidate['cases']}
        for case in original['cases']:
            self.assertEqual(indexed[case['id']],case)
        self.assertEqual(original['limits'],candidate['limits'])
        self.assertGreater(len(candidate['cases']),len(original['cases']))
        for version in ['v4','v5','v6']:
            revision=json.loads((ROOT/('models/suites/semantic-proposal-'+version+'.json')).read_text())
            self.assertEqual(revision['cases'],candidate['cases'])
            self.assertEqual(revision['limits'],candidate['limits'])
        data=json.loads((ROOT/'models/datasets/invocation-extraction-v2.json').read_text())
        self.assertEqual(len(data['cases']),10)
        self.assertEqual(LIVE.thresholds(data)['field_correctness']['minimum'],10000)
        self.assertEqual(LIVE.thresholds(data)['schema_conformance']['minimum_samples'],10)

    def samples(self):
        expected=dict(action='read',target='known.txt',ambiguous=False)
        binding=dict(invocation=dict(invocation_id='sample'),output_digest='a'*64)
        response=dict(status='VERIFIED',result=expected,parsed=dict(result=expected),verification=dict(validity='VALID'),
                      trace=dict(accounting=dict(total_input_tokens=4000),replay=dict(schema_version='1.0',output=binding)),
                      usage=[dict(reported=dict(output_tokens=20))])
        response['trace']['accounting']['count']=dict(profile=dict(renderer='fixture-renderer',template_version='8',generation_profile='a'*64))
        return [dict(expected=copy.deepcopy(expected),response=copy.deepcopy(response),exit_code=0,latency_ms=100) for _ in range(10)]

    def test_schema_valid_wrong_values_fail_semantic_and_unsupported_claim_metrics(self):
        samples=self.samples()
        probes=dict(semantic_rejection=True,context_overflow=True,provider_failure=True,cancellation=True,budget=True)
        before=LIVE.measurements(samples,probes,1000,2000000000)
        self.assertEqual(before['schema_conformance'],LIVE.rate(10,10))
        self.assertEqual(before['field_correctness'],LIVE.rate(30,30))
        samples[0]['response']['parsed']['result']['target']='invented.txt'
        samples[0]['response']['status']='REJECTED';samples[0]['exit_code']=1
        after=LIVE.measurements(samples,probes,1000,2000000000)
        self.assertEqual(after['schema_conformance'],LIVE.rate(10,10))
        self.assertEqual(after['field_correctness'],LIVE.rate(29,30))
        self.assertEqual(after['unsupported_claim_rate'],LIVE.rate(1,10))
        for sample in samples:
            sample['response']={};sample['exit_code']=1
        missing=LIVE.measurements(samples,probes,1000,2000000000)
        self.assertEqual(missing['schema_conformance'],LIVE.rate(0,10))
        self.assertEqual(missing['output_tokens'],LIVE.rate(0,1))
        self.assertEqual(missing['unsupported_claim_rate'],LIVE.rate(0,10))

    def test_missing_runtime_or_invalid_output_cannot_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);profile=root/'profile.json';profile.write_text('{"runtime_model":"unavailable"}')
            args=SimpleNamespace(output=root/'evidence',installed_profile=profile,runtime='unavailable')
            with patch.object(LIVE,'fingerprint',return_value={'fixture':'unchanged'}),patch.object(LIVE,'command',side_effect=OSError('missing runtime')):
                report=LIVE.run(args)
            self.assertEqual(report['status'],'FAIL')
            self.assertEqual(report['adapters'],[])
            with self.assertRaises(ValueError):LIVE.run(args)
            args.output=ROOT/'crates/forbidden-live-evidence'
            with self.assertRaises(ValueError):LIVE.run(args)

    def test_orchestration_rejects_evaluator_probe_failure_and_source_drift(self):
        # This is a control-flow fixture, never retained as model/live acceptance.
        for mode in ['pass', 'evaluator-failure', 'evaluator-exit-failure', 'evaluator-string-approval',
                     'evaluator-gates-failure', 'evaluator-fixture', 'probe-failure', 'source-drift',
                     'replay-empty', 'replay-wrong-binding', 'replay-with-result', 'replay-missing-trace',
                     'replay-exit-failure', 'runtime-drift-before', 'runtime-drift-after', 'context-parameter-mismatch', 'cpu-thread-pass', 'cpu-thread-mismatch']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root=Path(temp);binary=root/'fixture-binary';binary.write_bytes(b'fixture')
                profile=json.loads((ROOT/'models/qwen3-4b-canonical-v3.json').read_text())
                profile.update(artifact_digest='sha256:'+'a'*64, runtime_model='fixture:model', runtime_version='fixture', template_digest='sha256:'+'b'*64)
                if mode.startswith('cpu-thread-'):profile['resources']['cpu_threads']=4
                profile_path=root/'profile.json';profile_path.write_text(json.dumps(profile))
                args=SimpleNamespace(output=root/'evidence', installed_profile=profile_path,
                    runtime='fixture-runtime',service='fixture-service',local_endpoint='http://localhost:1',chat_endpoint='http://localhost:2')
                def invoke(binary, config, invocation, operation='invoke'):
                    name=config.parent.name
                    admitted=json.loads(config.read_text())
                    self.assertEqual(admitted['generation']['structured_output_mode'],'JSON')
                    self.assertEqual(admitted['capability']['structured_output_modes'],['JSON'])
                    self.assertEqual(admitted['registered_role'],'FILE_REQUEST_EXTRACTION_V1')
                    self.assertEqual(set(admitted['output']['shape']['fields']),{'action','target','ambiguous'})
                    if operation=='cancel': return 0,dict(status='CANCELLED')
                    if operation=='replay':
                        binding=self.samples()[0]['response']['trace']['replay']['output']
                        receipt=dict(schema_version='1.0',admitted_invocation=binding['invocation'],output=binding,diagnostic=None)
                        if mode=='replay-empty':receipt={}
                        if mode=='replay-wrong-binding':receipt['output']=dict(binding,output_digest='b'*64)
                        if mode=='replay-with-result':receipt['result']=dict(action='read')
                        return (1 if mode=='replay-exit-failure' else 0),receipt
                    for probe, response in {
                        'provider_failure':dict(status='FAILED'), 'cancellation':dict(diagnostic='CG_CANCELLED'),
                        'context_overflow':dict(diagnostic='CG_RENDERING_FAILED'),
                        'semantic_rejection':dict(status='REJECTED')}.items():
                        if name.endswith(probe):
                            if mode=='probe-failure' and probe=='semantic_rejection':return 0,dict(status='VERIFIED')
                            return 1,response
                    if invocation=='second':return 1,dict(diagnostic='CG_BUDGET_EXCEEDED')
                    a=json.loads(config.read_text());expected=json.loads(Path(a['oracle_file']).read_text())
                    response=self.samples()[0]['response'];response['parsed']['result']=expected
                    if mode=='replay-missing-trace':response['trace'].pop('replay')
                    return 0,response
                inspections=0
                def command(argv, **kwargs):
                    nonlocal inspections
                    if 'inspect' in argv:
                        inspections+=1
                        changed=(mode=='runtime-drift-before' and inspections>1) or (mode=='runtime-drift-after' and inspections>2)
                        cache='f16' if changed else 'q8_0'
                        return json.dumps(dict(image='sha256:fixture',environment=['OLLAMA_KV_CACHE_TYPE='+cache,'UNRELATED_SECRET=do-not-retain'],memory_limit=0,cpu_quota=0))
                    if 'ollama' in argv:return ''
                    if 'sys.argv[1]' in argv[-2]: return json.dumps(dict(id='gpt2:qwen2',parameters=('num_ctx 1024' if mode=='context-parameter-mismatch' else 'num_ctx 16384')+('\nnum_thread 8' if mode=='cpu-thread-mismatch' else '\nnum_thread 4')))
                    return json.dumps(dict(models=[dict(name=profile['runtime_model'],size=2000000000)]))
                fingerprints=[{'fixture':'before'},{'fixture':'after' if mode=='source-drift' else 'before'}]
                decision=SimpleNamespace(returncode=1 if mode=='evaluator-exit-failure' else 0,
                    stdout=json.dumps(dict(eligible_for_promotion=('true' if mode=='evaluator-string-approval' else mode!='evaluator-failure'),
                                           gates_passed=mode!='evaluator-gates-failure',
                                           evidence='fixture' if mode=='evaluator-fixture' else 'live')))
                with patch.object(LIVE,'fingerprint',side_effect=fingerprints), patch.object(LIVE,'build',return_value=(binary,binary)), \
                     patch.object(LIVE,'runtime_identity',return_value={}),patch.object(LIVE,'resources',side_effect=[(1000000,'fixture'),(2000000,'fixture')]*2), \
                     patch.object(LIVE,'command',side_effect=command),patch.object(LIVE,'invoke',side_effect=invoke), \
                     patch.object(LIVE.subprocess,'run',return_value=decision),patch('builtins.print'):
                    report=LIVE.run(args)
                self.assertEqual(report['status'], 'LIVE_ROLE_PASS' if mode in ['pass','cpu-thread-pass'] else 'FAIL' if mode=='source-drift' or mode.startswith('runtime-drift-') or mode in ['context-parameter-mismatch','cpu-thread-mismatch'] else 'NOT_COMPLETE')
                self.assertNotIn('UNRELATED_SECRET',str(report.get('runtime_configuration',{})))
                self.assertNotIn('do-not-retain',str(report.get('runtime_configuration',{})))
                self.assertEqual(len(report['adapters']),0 if mode.startswith('runtime-drift-') or mode in ['context-parameter-mismatch','cpu-thread-mismatch'] else 2)
                if report['adapters']:self.assertEqual(report['adapters'][0]['evaluator_exit_code'],decision.returncode)
                if mode.startswith('replay-'):
                    self.assertTrue(all(not a['probes']['replay'] for a in report['adapters']))
                self.assertEqual(report['model_promotion']['gateway_capability_catalog'],'NOT_RUN')
                if report['adapters']:
                    provenance=json.loads((args.output/'local-profile.json').read_text())['provenance']
                    self.assertEqual(provenance['renderer']['revision'],'8')
                    self.assertEqual(provenance['renderer']['digest'],LIVE.sha(LIVE.compact({name:LIVE.sha((ROOT/name).read_bytes()) for name in ['crates/gateway-daemon/src/model_adapters.rs','crates/gateway-daemon/src/prompt_renderers.rs','crates/gateway-daemon/src/file_request_extraction_v1.txt']})))

    def test_runtime_provenance_build_and_subprocess_errors_fail_closed(self):
        with patch.object(LIVE,'command',return_value='{"runtime_version":"different"}'):
            with self.assertRaises(ValueError):LIVE.runtime_identity('fixture',dict(runtime_version='fixed'))
        with tempfile.TemporaryDirectory() as temp:
            result=SimpleNamespace(returncode=1,stdout='',stderr='compile failure')
            with patch.object(LIVE.subprocess,'run',return_value=result):
                with self.assertRaises(ValueError):LIVE.build(Path(temp))

    def test_runner_boundaries_and_cli_are_covered_with_explicit_subprocess_fixtures(self):
        self.assertIn('Cargo.lock',LIVE.fingerprint())
        result=SimpleNamespace(returncode=0,stdout='{"status":"fixture"}',stderr='')
        with patch.object(LIVE.subprocess,'run',return_value=result) as call:
            self.assertEqual(LIVE.command(['fixture']),result.stdout)
            self.assertEqual(LIVE.invoke(Path('/fixture'),Path('/config'),'id'),(0,dict(status='fixture')))
            self.assertEqual(call.call_args.kwargs['timeout'],210)
        with patch.object(LIVE,'command',return_value='usage_usec 12345\nuser_usec 123\n'):
            self.assertEqual(LIVE.resources('fixture')[0],12345)
        with tempfile.TemporaryDirectory() as temp:
            lines=[dict(target=dict(name=name),executable='/fixture/'+name) for name in ['cg-invoke','qualify-invocation']]
            result=SimpleNamespace(returncode=0,stdout='\n'.join(json.dumps(v) for v in lines),stderr='')
            with patch.object(LIVE.subprocess,'run',return_value=result):
                self.assertEqual(LIVE.build(Path(temp)),(Path('/fixture/cg-invoke'),Path('/fixture/qualify-invocation')))
        argv=['qualify-invocation-live.py','--installed-profile','/fixture/profile','--local-endpoint','http://localhost:1',
              '--chat-endpoint','http://localhost:2','--output','/fixture/output']
        for status, expected in [('LIVE_ROLE_PASS',0),('FAIL',1)]:
            with patch('sys.argv',argv),patch.object(LIVE,'run',return_value=dict(status=status)),patch('builtins.print'):
                self.assertEqual(LIVE.main(),expected)


if __name__=='__main__':
    unittest.main()
