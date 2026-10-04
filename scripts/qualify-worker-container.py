#!/usr/bin/env python3
"""Execute frozen CPU-model work inside a credential-free, resource-bounded container."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('cpu_pipeline', ROOT / 'services/offline-learning/pipeline.py')
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)
fixture = json.loads((ROOT / 'tests/fixtures/epic03-ml-v1/experiment.json').read_text())
trained = pipeline.train(fixture['training'])
name = 'cg-epic03-worker-' + uuid.uuid4().hex[:12]
image = name + ':qualification'
try:
    subprocess.run(['docker', 'build', '-q', '-f', str(ROOT / 'services/cognitive-worker/Dockerfile'), '-t', image, str(ROOT)], check=True, capture_output=True)
    command = ['docker', 'run', '--name', name, '--network=none', '--read-only', '--memory=256m', '--memory-swap=256m',
               '--cpus=1', '--pids-limit=32', '--cap-drop=ALL', '--security-opt=no-new-privileges', '-i', image]
    request = {'kind': 'MODEL', 'artifact_json': json.dumps(trained), 'row': fixture['test'][1]}
    started = time.perf_counter_ns()
    result = subprocess.run(command, input=json.dumps(request), text=True, capture_output=True, timeout=30, check=True)
    elapsed = time.perf_counter_ns() - started
    proposal = json.loads(result.stdout)
    assert proposal['label'] == fixture['test'][1]['label'] and proposal['disposition'] == 'PROPOSAL'
    inspected = json.loads(subprocess.check_output(['docker', 'inspect', name], text=True))[0]
    config = inspected['HostConfig']
    assert config['Memory'] == 268435456 and config['NanoCpus'] == 1000000000 and config['PidsLimit'] == 32
    assert config['NetworkMode'] == 'none' and config['ReadonlyRootfs'] and config['CapDrop'] == ['ALL']
    assert inspected['Config']['User'] == '10001:10001' and not inspected['Mounts']
    assert not any('PASSWORD=' in x or 'CG_COGNITIVE_TEST_DATABASE=' in x for x in inspected['Config']['Env'])
    report = {'schema_version': 1, 'status': 'PASS', 'image_digest': inspected['Image'], 'model_digest': trained['artifact_digest'],
              'input_digest': pipeline.digest(request), 'proposal': proposal, 'worker_uid': inspected['Config']['User'],
              'limits': {k: config[k] for k in ('Memory', 'MemorySwap', 'NanoCpus', 'PidsLimit', 'NetworkMode', 'ReadonlyRootfs', 'CapDrop', 'SecurityOpt')},
              'wall_ns': elapsed, 'authority_credentials': False, 'production_mounts': False}
    with Path(sys.argv[1]).open('x') as output:
        json.dump(report, output, indent=2)
        output.write('\n')
    print('EPIC-03 isolated container inference PASS')
finally:
    subprocess.run(['docker', 'rm', '--force', name], capture_output=True)
    subprocess.run(['docker', 'image', 'rm', image], capture_output=True)
