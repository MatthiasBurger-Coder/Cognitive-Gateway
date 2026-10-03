#!/usr/bin/env python3
"""Real CPU qualification and container/adapter replay on an EMPTY model registry.
Retains model volumes and auditable evidence; never deletes existing state.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def run(*args, input=None):
    return subprocess.check_output(args, cwd=ROOT, text=True, input=input)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'status': 'RUNNING', 'steps': [], 'source_revision': run('git', 'rev-parse', 'HEAD').strip()}
    compose = ['docker', 'compose', '-f', 'compose.model.yaml']
    def record(name, data):
        (args.output / (name + '.json')).write_text(json.dumps(data, indent=2) + '\n')
        report['steps'].append(name)
        print(name, flush=True)
    def model(*args):
        return json.loads(run('scripts/model.sh', *args))
    try:
        run('scripts/model.sh', 'start')
        state = model('inspect')
        if state['profiles']:
            raise RuntimeError('Proof needs an empty registry; choose a new CG_MODEL_PROJECT_NAME and port. Existing state was preserved.')
        record('install', model('install', '/models/qwen3-8b-q4.json'))
        record('cpu-benchmark', model('test', 'qwen3-8b-q4'))
        record('promotion', model('promote', 'qwen3-8b-q4'))
        record('ready', json.loads(run('scripts/model.sh', 'ready')))
        suite = json.loads((ROOT / 'models/suites/semantic-proposal-v1.json').read_text())
        request = {'schema_version': '1.0', 'role': 'semantic-interpreter', 'input_contract': suite['input_contract'],
                   'output_contract': suite['output_contract'], 'prompt': suite['cases'][0]['prompt'], 'output_schema': suite['output_schema']}
        run(*compose, '--profile', 'gateway', 'build', 'gateway')
        proposal = json.loads(run(*compose, 'run', '--rm', '--no-deps', '-T', '--entrypoint', 'local-model-proposal', 'gateway', input=json.dumps(request)))
        assert proposal['kind'] == 'proposal' and proposal['proposal'] == suite['cases'][0]['expected']
        record('rust-container-proposal', proposal)
        # A second manifest proves profile replacement with the same Gateway image.
        replacement = json.loads((ROOT / 'models/qwen3-8b-q4.json').read_text())
        replacement['model_id'] = 'qwen3-8b-q4-replacement-proof'
        run(*compose, 'exec', '-T', 'model-service', 'python', '-c',
            "import sys; open('/tmp/replacement.json','w').write(sys.stdin.read())", input=json.dumps(replacement))
        record('replacement-install', model('install', '/tmp/replacement.json'))
        assert model('inspect')['aliases']['semantic-interpreter'] == 'qwen3-8b-q4'
        record('replacement-qualification', model('test', replacement['model_id']))
        record('replacement-promotion', model('promote', replacement['model_id']))
        replaced = json.loads(run(*compose, 'run', '--rm', '--no-deps', '-T', '--entrypoint', 'local-model-proposal', 'gateway', input=json.dumps(request)))
        assert replaced['model_id'] == replacement['model_id']
        record('replacement-proposal', replaced)
        record('rollback', model('rollback', 'semantic-interpreter'))
        assert model('inspect')['aliases']['semantic-interpreter'] == 'qwen3-8b-q4'
        # Recreate service containers while preserving artifact and state volumes.
        run(*compose, 'up', '-d', '--force-recreate', '--wait', '--wait-timeout', '360')
        record('persistent-ready', json.loads(run('scripts/model.sh', 'ready')))
        run(*compose, 'stop', 'runtime')
        try:
            endpoint = f"http://127.0.0.1:{os.environ.get('CG_MODEL_SERVICE_PORT', '8091')}"
            try:
                urllib.request.urlopen(urllib.request.Request(endpoint + '/v1/infer', data=json.dumps(request).encode(), headers={'Content-Type': 'application/json'}), timeout=15)
            except urllib.error.HTTPError as error:
                failure = json.load(error)
                assert error.code == 503 and failure['error'] == 'runtime_unavailable'
                record('unavailable-fallback', failure)
            else:
                raise AssertionError('Runtime unavailability was not fail-closed')
            assessment = json.loads(run(*compose, 'run', '--rm', '--no-deps', '-T',
                                        '-v', str(ROOT / 'tests/fixtures/declarative-cli') + ':/fixtures:ro',
                                        'gateway', 'assess', '--context', '/fixtures/context.json', '--json'))
            record('independent-core', assessment)
        finally:
            run(*compose, 'up', '-d', '--wait', '--wait-timeout', '360', 'runtime')
        record('registry-audit', model('inspect'))
        record('containers', [json.loads(line) for line in run(*compose, 'ps', '--format', 'json').splitlines()])
        report['status'] = 'PASS'
    except Exception as error:
        report.update(status='FAIL', error=str(error))
        raise
    finally:
        (args.output / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
