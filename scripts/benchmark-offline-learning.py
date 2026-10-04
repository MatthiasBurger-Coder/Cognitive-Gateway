#!/usr/bin/env python3
"""Measure the real CPU reference experiment; retain model and split lineage."""
import importlib.util
import json
import os
from pathlib import Path
import platform
import resource
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('offline_pipeline', ROOT / 'services/offline-learning/pipeline.py')
cases = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cases)
pipeline = cases
fixture_path = ROOT / "tests/fixtures/epic03-ml-v1/experiment.json"
fixture = json.loads(fixture_path.read_text())
started = time.perf_counter_ns()
cpu_start = time.process_time_ns()
req = fixture["training"]
trained = pipeline.train(req)
evaluated = pipeline.evaluate({**trained, 'rows': fixture["test"], 'profile': fixture["profile"]})
# Independent replay with reversed source iteration and a second search contract.
replayed = pipeline.train({**req, 'rows': list(reversed(req['rows']))})
random_req = {**req, 'plan': {**req['plan'], 'search': 'random', 'cv': 'group', 'budget': 3}}
random_run = pipeline.train(random_req)
artifact = trained['artifact']
normal = fixture["test"]
shifted = json.loads(json.dumps(fixture["test"]))
for row in shifted:
    row['features']['health'] *= 100
monitoring = pipeline.drift(artifact, shifted, [1 - r['label'] for r in shifted], {'min_f1': 0.9, 'max_brier': 0.1, 'max_ood_rate': 0})
report = {'schema_version': 1, 'suite': 'EPIC03-ML-v1', 'status': evaluated['status'], 'dataset': fixture['dataset'], 'dataset_digest': pipeline.digest(fixture), 'fixture_path':str(fixture_path.relative_to(ROOT)),
          'model': trained, 'evaluation': evaluated, 'reproduced': trained == replayed,
          'grid_trials': len(artifact['trials']), 'random_trials': len(random_run['artifact']['trials']),
          'drift': monitoring, 'task_metrics': {'regression': pipeline.regression([1, 2, 3], [1.1, 1.9, 3]),
                                             'ranking': pipeline.ranking({'a', 'b'}, ['a', 'c', 'b'], 3)},
          'performance': {'wall_ns': time.perf_counter_ns() - started, 'cpu_ns': time.process_time_ns() - cpu_start,
                          'peak_process_rss_bytes': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (1 if sys.platform == 'darwin' else 1024),
                          'hardware': platform.machine(), 'platform': platform.platform(), 'python': platform.python_version(),
                          'model_artifact_bytes': len(pipeline.canonical(artifact)), 'external_provider_calls': 0,
                          'gpu': {'status': 'NOT_USED', 'reason': 'CPU reference algorithm'},
                          'measurement_scope': 'this process, including imports and reference evaluation; synthetic data; no production SLA'}}
with Path(sys.argv[1]).open('x') as output:
    json.dump(report, output, indent=2, allow_nan=False)
    output.write('\n')
assert evaluated['status'] == 'PASS' and trained == replayed and monitoring['action'] != 'RETAIN'
print('EPIC-03 real CPU experiment PASS')
