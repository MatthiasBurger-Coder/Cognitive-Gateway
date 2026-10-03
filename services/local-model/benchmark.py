#!/usr/bin/env python3
"""Proposal-only benchmarks. Evidence is observational and never promotes a model."""
import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import sys
import time

from service import ModelError, Runtime, digest, load, now, validate
from signals import CognitiveSignalAdapter

ROOT = Path(__file__).resolve().parents[2]
TASKS = {'classification', 'ranking', 'extraction', 'matching'}


def fixture_output(task, value):
    """Small independent rules; never reads dataset labels or expected outputs."""
    if task == 'classification':
        words = value['text'].lower().split()
        if not words:
            return {'label': 'unknown'}
        return {'label': 'read' if words[0] in ('read', 'open') else
                'write' if words[0] in ('update', 'write') else 'unknown'}
    if task == 'ranking':
        query = set(value['query'].lower().split())
        ranked = sorted(value['candidates'], key=lambda c: (
            -len(query & set(c['text'].lower().split())), c['id']))
        return {'ids': [c['id'] for c in ranked]}
    if task == 'extraction':
        targets = [word.strip('.,') for word in value['text'].split()
                   if '.' in word.strip('.,') and '/' not in word]
        return {'target': targets[0] if targets else None}
    if task == 'matching':
        # Explicit fact equality is the fixture baseline, never permission to act.
        return {'ids': sorted(c['id'] for c in value['candidates']
                              if c['facts'] == value['facts'])}
    raise ModelError('task_unsupported')


class FixtureAdapter:
    acceleration = 'cpu'

    def check(self, profile):
        if (profile['runtime'] != 'deterministic-fixture'
                or profile['runtime_version'] != '1.0'
                or profile['artifact_digest'] != source_digest(Path(__file__))
                or profile['template_digest'] != digest('fixture-json/1.0')):
            raise ModelError('provenance_changed')
        return {key: profile[key] for key in ('artifact_digest', 'runtime_version', 'template_digest')}

    def generate(self, profile, request, system, cold=False):
        value = json.loads(request['prompt'])
        started = time.monotonic()
        proposal = fixture_output(value['task'], value['input'])
        validate(request['output_schema'], proposal)
        return {'schema_version': '1.0', 'kind': 'proposal', 'model_id': profile['model_id'],
                'artifact_digest': profile['artifact_digest'], 'proposal': proposal,
                'metrics': {'latency_seconds': time.monotonic() - started,
                            'load_seconds': None, 'tokens_per_second': None},
                'provenance': {'model_version': profile['model_version'], 'runtime': profile['runtime'],
                               'runtime_version': profile['runtime_version'],
                               'runtime_configuration': {'acceleration': 'cpu', 'algorithm': 'fixture-rules/1.0'},
                               'prompt_version': profile['prompt_version'], 'system_digest': digest(system),
                               'template_digest': profile['template_digest'],
                               'input_contract': request['input_contract'], 'output_contract': request['output_contract']}}

    def memory(self, profile):
        return {'resident_model_bytes': None, 'vram_bytes': None,
                'source': 'not-applicable-in-process-fixture'}


def source_digest(path):
    return 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()


def fixture_profile(dataset):
    return {'schema_version': '1.0', 'model_id': 'deterministic-fixture-v1',
            'model_family': 'rules-baseline', 'model_version': '1.0',
            'artifact_digest': source_digest(Path(__file__)), 'runtime': 'deterministic-fixture',
            'runtime_version': '1.0', 'runtime_model': 'in-process', 'quantization': 'none',
            'role': 'cognitive-signals', 'capabilities': sorted(TASKS),
            'supported_input_contracts': [dataset['input_contract']],
            'supported_output_contracts': [task['output_contract'] for task in dataset['tasks'].values()],
            'structured_output_support': True, 'context_limit': 4096,
            'resources': {'cpu': True, 'gpu': 'none'}, 'prompt_version': 'fixture-json/1.0',
            'template_digest': digest('fixture-json/1.0'), 'qualification_suite': dataset['id'],
            'qualification_status': 'unqualified', 'lifecycle': 'candidate',
            'provenance': {'source': 'services/local-model/benchmark.py', 'revision': source_digest(Path(__file__))}}


def validate_inputs(dataset, profile, rounds):
    validate(load(ROOT / 'schemas/model-benchmark-dataset.schema.json'), dataset)
    validate(load(ROOT / 'schemas/model-profile.schema.json'), profile)
    if not 1 <= rounds <= 100:
        raise ModelError('rounds_invalid')
    if dataset['input_contract'] not in profile['supported_input_contracts']:
        raise ModelError('contract_unsupported')
    ids = [case['id'] for case in dataset['cases']]
    if len(set(ids)) != len(ids):
        raise ModelError('dataset_duplicate_case')
    for case in dataset['cases']:
        if case['task'] not in dataset['tasks']:
            raise ModelError('task_unsupported')
        task = dataset['tasks'][case['task']]
        if case['task'] not in profile['capabilities'] or task['output_contract'] not in profile['supported_output_contracts']:
            raise ModelError('contract_unsupported')
        validate(task['input_schema'], case['input'])
        validate(task['output_schema'], case['expected'])
    for key in ('artifact_digest', 'template_digest'):
        if profile[key] is None:
            raise ModelError('provenance_required')


def memory(adapter, profile):
    if hasattr(adapter, 'memory'):
        return adapter.memory(profile)
    resident = next((m for m in adapter.call('/api/ps')['models']
                     if m['name'] == profile['runtime_model']), None)
    if resident is None:
        raise ModelError('model_not_resident')
    return {'resident_model_bytes': resident['size'], 'vram_bytes': resident.get('size_vram', 0),
            'source': 'ollama-api-ps-resident-snapshot'}


def summarize(samples):
    result = {}
    for phase in ('cold', 'warm'):
        group = [s for s in samples if s['phase'] == phase]
        elapsed = sum(s['wall_seconds'] for s in group)
        latencies = sorted(s['wall_seconds'] for s in group)
        tokens = [s['result']['metrics']['tokens_per_second'] for s in group if 'result' in s
                  and s['result']['metrics']['tokens_per_second'] is not None]
        result[phase] = {'attempts': len(group), 'successes': sum('result' in s for s in group),
                         'exact_match_accuracy': sum(s.get('correct', False) for s in group) / len(group) if group else None,
                         'latency_p50_seconds': statistics.median(latencies) if group else None,
                         'latency_p95_seconds': latencies[math.ceil(len(group) * .95) - 1] if group else None,
                         'successful_requests_per_second': sum('result' in s for s in group) / elapsed if elapsed else None,
                         'mean_generated_tokens_per_second': statistics.mean(tokens) if tokens else None}
    result['quality_by_task'] = {task: {'attempts': len(group),
                                       'exact_match_accuracy': sum(s.get('correct', False) for s in group) / len(group)}
                                 for task in sorted(TASKS)
                                 if (group := [s for s in samples if s['task'] == task and s['phase'] == 'warm'])}
    result['memory'] = {key: max(values) if values else None
                        for key in ('resident_model_bytes', 'vram_bytes')
                        for values in [[s['memory'][key] for s in samples if 'memory' in s and s['memory'][key] is not None]]}
    return result


def run(dataset, profile, adapter, rounds=3):
    """No registry access: an injected adapter can be replaced without core changes."""
    validate_inputs(dataset, profile, rounds)
    evidence_kind = 'deterministic-fixture' if isinstance(adapter, FixtureAdapter) else 'model'
    report = {'schema_version': '1.0', 'status': 'running', 'created_at': now(),
              'evidence_kind': evidence_kind, 'product_claim': False,
              'dataset': {'id': dataset['id'], 'version': dataset['version'], 'digest': digest(dataset)},
              'inputs': {'dataset': copy.deepcopy(dataset), 'profile': copy.deepcopy(profile), 'warm_rounds': rounds},
              'implementation': {name: source_digest(Path(__file__).parent / name)
                                 for name in ('benchmark.py', 'service.py', 'signals.py')},
              'schemas': {name: source_digest(ROOT / 'schemas' / name)
                          for name in ('model-profile.schema.json', 'model-benchmark-dataset.schema.json')},
              'hardware': {'platform': platform.platform(), 'cpu': platform.processor(), 'cpu_count': os.cpu_count(),
                           'cpuinfo': Path('/proc/cpuinfo').read_text() if Path('/proc/cpuinfo').exists() else None,
                           'meminfo': Path('/proc/meminfo').read_text() if Path('/proc/meminfo').exists() else None},
              'acceleration': adapter.acceleration, 'samples': [],
              'cold_definition': 'model unload before first request; OS file cache is not flushed' if evidence_kind == 'model'
                                 else 'first fixture call; no model load or GPU measurement'}
    try:
        report['runtime_identity'] = adapter.check(profile)
        signals = CognitiveSignalAdapter(adapter, profile, dataset)
        cases = [(dataset['cases'][0], 'cold', 0)] + [(case, 'warm', iteration)
                 for iteration in range(1, rounds + 1) for case in dataset['cases']]
        for case, phase, iteration in cases:
            sample = {'case_id': case['id'], 'task': case['task'], 'phase': phase, 'iteration': iteration}
            started = time.monotonic()
            try:
                result = signals.infer(case['task'], case['input'], cold=phase == 'cold')
                sample['wall_seconds'] = time.monotonic() - started
                observed = memory(adapter, profile)
                if adapter.acceleration == 'cpu' and observed['vram_bytes'] not in (None, 0):
                    raise ModelError('cpu_gate_failed')
                if adapter.acceleration != 'cpu' and not (observed['vram_bytes'] or 0) > 0:
                    raise ModelError('hardware_unavailable')
                sample.update(result=result, correct=result['proposal'] == case['expected'], memory=observed)
            except (ModelError, ValueError, KeyError, TypeError) as error:
                sample['wall_seconds'] = time.monotonic() - started
                sample['error'] = error.code if isinstance(error, ModelError) else 'adapter_response_invalid'
            report['samples'].append(sample)
            # Transport/hardware failure is a fallback outcome, never substitute CPU or another model.
            if sample.get('error') in ('runtime_unavailable', 'hardware_unavailable', 'provenance_changed'):
                report['error'] = sample['error']
                break
        adapter.check(profile)
        report['status'] = 'failed' if any('error' in s or not s['correct'] for s in report['samples']) else 'completed'
    except ModelError as error:
        report.update(status='unavailable' if error.code in ('runtime_unavailable', 'model_missing') else 'failed',
                      error=error.code)
    if report.get('error') in ('hardware_unavailable', 'runtime_unavailable', 'model_missing'):
        report['status'] = 'unavailable'
    report['summary'] = summarize(report['samples'])
    report['finished_at'] = now()
    report['fallback'] = 'deterministic-core' if report['status'] != 'completed' else None
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dataset', type=Path, default=ROOT / 'models/datasets/cognitive-signals-v1.json')
    parser.add_argument('--adapter', choices=('fixture', 'ollama'), default='fixture')
    parser.add_argument('--profile', type=Path, help='Installed immutable profile JSON; required for Ollama')
    parser.add_argument('--acceleration', choices=('cpu', 'nvidia', 'amd', 'vulkan'), default='cpu')
    parser.add_argument('--endpoint', default='http://127.0.0.1:11434')
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('--warm-rounds', type=int, default=3)
    parser.add_argument('--output', required=True, type=Path, help='New report file; existing evidence is never overwritten')
    args = parser.parse_args()
    if (args.adapter == 'ollama' and not args.profile) or (args.adapter == 'fixture' and args.profile):
        parser.error('--profile is required only for the Ollama adapter')
    try:
        dataset = load(args.dataset)
        profile = load(args.profile) if args.profile else fixture_profile(dataset)
        adapter = FixtureAdapter() if args.adapter == 'fixture' else Runtime(args.endpoint, args.timeout, args.acceleration)
        if args.adapter == 'fixture' and args.acceleration != 'cpu':
            raise ModelError('fixture_cpu_only')
        report = run(dataset, profile, adapter, args.warm_rounds)
        report['runtime_endpoint'] = args.endpoint if args.adapter == 'ollama' else None
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open('x') as stream:
            json.dump(report, stream, indent=2, allow_nan=False)
            stream.write('\n')
        print(f"{report['status']}: {args.output}")
        return 0 if report['status'] == 'completed' else 1
    except (ModelError, ValueError, KeyError, TypeError, OSError) as error:
        print(error.code if isinstance(error, ModelError) else str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
