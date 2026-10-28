#!/usr/bin/env python3
"""Execute revision-bound EPIC-06 fixture qualification. Never invokes live models."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / 'tests/scripts/invocation-acceptance.json'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fingerprint(output):
    paths = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).decode().split('\0')
    return {p: digest(ROOT / p) for p in sorted(set(paths)) if p and (ROOT / p).is_file()
            and not (ROOT / p).resolve().is_relative_to(output)
            and not p.startswith('docs/evidence/')}


def validate_manifest(manifest):
    if set(manifest) != {'schema_version', 'criteria', 'gates'} or manifest['schema_version'] != '1.0':
        raise ValueError('Invalid qualification manifest version or fields')
    gates = manifest['gates']
    ids = [g['id'] for g in gates]
    if not ids or len(ids) != len(set(ids)):
        raise ValueError('Missing or duplicate gate IDs')
    for gate in gates:
        if set(gate) != {'id', 'command', 'rust_tests'} or not gate['command'].strip() or type(gate['rust_tests']) is not bool or not re.fullmatch(r'[a-z0-9-]+', gate['id']):
            raise ValueError('Invalid gate')
    criteria = manifest['criteria']
    if not criteria or len({c['id'] for c in criteria}) != len(criteria):
        raise ValueError('Missing or duplicate criteria')
    for criterion in criteria:
        if not criterion['requirement'].strip() or not criterion['gates'] or not set(criterion['gates']) <= set(ids) or not criterion['tests']:
            raise ValueError('Unmapped criterion')
        for test in criterion['tests']:
            path, name = test.split('::', 1)
            source = (ROOT / path).read_text()
            if not re.search(r'fn\s+' + re.escape(name) + r'\s*\(', source):
                raise ValueError('Missing mapped Rust test: ' + test)
    return manifest


def execute(command, output, env):
    with output.open('wb') as log:
        result = subprocess.run(['bash', '-eu', '-o', 'pipefail', '-c', command], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
    return result.returncode


def run(output, perform, manifest=None, executor=execute, snapshot=fingerprint):
    output = output.resolve()
    # Keeping evidence outside source directories prevents an output path from
    # excluding production files from the candidate fingerprint.
    if output == ROOT or (ROOT in output.parents and not output.is_relative_to(ROOT / 'target')):
        raise ValueError('Evidence must be outside checkout or under target/')
    manifest = validate_manifest(manifest if manifest is not None else json.loads(MANIFEST.read_text()))
    output.mkdir(parents=True, exist_ok=False)
    report = {'schema_version': '1.0', 'issue': 270, 'evidence_kind': 'fixture',
              'status': 'NOT_RUN', 'release_status': 'NOT_COMPLETE', 'model_promotion': 'NOT_RUN',
              'live_evidence': 'NOT_RUN',
              'limitations': ['Synthetic trusted hosts and loopback providers; no stochastic reproducibility claim.',
                              'Live role metrics and installed host composition require separate evidence.'],
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'source_sha256': snapshot(output), 'manifest_sha256': digest(MANIFEST),
              'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'gates': [{**g, 'status': 'NOT_RUN'} for g in manifest['gates']],
              'criteria': [{**c, 'status': 'NOT_RUN', 'level': 'COMPONENT'} for c in manifest['criteria']]}
    def save():
        (output / 'qualification.json').write_text(json.dumps(report, indent=2) + '\n')
    save()
    if not perform:
        return report
    env = os.environ.copy()
    env.update(EVIDENCE_DIR=str(output), CARGO_TERM_COLOR='never')
    env.pop('CG_MODEL_ADAPTER_LIVE_PROFILE', None)
    env.pop('CG_MODEL_ADAPTER_LIVE_OUTPUT', None)
    try:
        for index, gate in enumerate(report['gates']):
            log = output / f'{index:02d}-{gate["id"]}.log'
            print(f'Running {gate["id"]}', flush=True)
            code = executor(gate['command'], log, env)
            gate.update(exit_code=code, log=log.name, log_sha256=digest(log))
            # A successful cargo invocation with no executed tests is not proof.
            gate['tests_passed'] = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed;', log.read_text(errors='replace')))
            gate['status'] = 'PASS' if code == 0 and (not gate['rust_tests'] or gate['tests_passed'] > 0) else 'FAIL'
            save()
            if gate['status'] == 'FAIL':
                break
        statuses = {g['id']: g['status'] for g in report['gates']}
        test_logs = '\n'.join((output / g['log']).read_text(errors='replace')
                              for g in report['gates'] if g['rust_tests'] and 'log' in g)
        for criterion in report['criteria']:
            states = [statuses[g] for g in criterion['gates']]
            criterion['tests_verified'] = all(re.search(r'^test (?:[A-Za-z0-9_]+::)*' + re.escape(t.split('::', 1)[1]) + r' \.\.\. ok$', test_logs, re.MULTILINE) for t in criterion['tests'])
            criterion['status'] = 'VERIFIED' if all(s == 'PASS' for s in states) and criterion['tests_verified'] else 'NOT_VERIFIED'
        report['status'] = 'COMPONENT_PASS' if all(g['status'] == 'PASS' for g in report['gates']) and all(c['status'] == 'VERIFIED' for c in report['criteria']) else 'FAIL'
        if snapshot(output) != report['source_sha256']:
            report['status'] = 'FAIL'
            report['source_drift'] = True
            for criterion in report['criteria']:
                criterion['status'] = 'NOT_VERIFIED'
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report.update(status='FAIL', runner_error=type(error).__name__)
    finally:
        report['finished_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        save()
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = run(args.output, args.run)
    print(f'{report["status"]}; release={report["release_status"]}; {args.output / "qualification.json"}')
    return 0 if report['status'] == 'COMPONENT_PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
