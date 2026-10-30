#!/usr/bin/env python3
"""Qualify EPIC-07 connectors with separate synthetic, live and full quality evidence."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / 'tests/scripts/connector-acceptance.json'


def cargo(package, *targets):
    return ['cargo', 'test', '-p', package, *targets, '--locked', '--', '--test-threads=1']


GATES = {
    **{name: cargo('gateway-application', '--test', 'connector_' + name)
       for name in ('manifest', 'admission', 'policy', 'resilience')},
    'runtime-lib': ['cargo', 'test', '-p', 'gateway-daemon', '--lib', '--locked',
                    'mcp_client::', '--', '--test-threads=1'],
    'runtime': cargo('gateway-daemon', '--test', 'mcp_client'),
    'github': cargo('gateway-daemon', '--test', 'github_connector'),
    'providers': cargo('gateway-daemon', '--test', 'conformance_connector'),
    'proposals': cargo('gateway-daemon', '--test', 'connector_proposals'),
    'architecture': ['bash', 'tests/scripts/check-architecture.sh'],
    'format': ['cargo', 'fmt', '--all', '--check'],
}
BLOCKERS = {
    'E07-28': 'Full workspace quality evidence requires --release.',
    'E07-29': 'Run all applicable production coverage gates in tests/scripts/quality-gates.json against this candidate.',
    'E07-31': 'Controlled live action and independent verification require --release --live-repository.',
    'E07-38': 'Live normalized dispatch/result/reconciliation contract evidence requires --release.',
}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def fingerprint():
    paths = subprocess.check_output(
        ['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).decode().split('\0')
    return {p: sha256(ROOT / p) for p in sorted(set(paths)) if p and (ROOT / p).is_file()}


def passed_tests(log):
    """Only actual libtest success records count; discovery and zero-test exits do not."""
    return re.findall(r'^test (\S+) \.\.\. ok$', log, re.M)


def reconcile(manifest, gates):
    rows = manifest['requirements']
    if [r['id'] for r in rows] != [f'E07-{i:02}' for i in range(1, 40)]:
        raise ValueError('All 30 original, three qualification and six parent cross-plane criteria are required in order')
    results = []
    for row in rows:
        gate = gates.get(row['gate'], {})
        test = row['test']
        matches = [name for name in gate.get('passed_tests', [])
                   if name == test or name.endswith('::' + str(test))]
        verified = gate.get('status') == 'PASS' and (test is None or len(matches) == 1)
        results.append({**row, 'status': 'VERIFIED' if verified else 'BLOCKED',
                        'evidence_level': ('live external effect' if row['gate'] == 'live-effects' else
                                           'full quality and coverage' if row['gate'] == 'quality' else
                                           'synthetic component with shipped binaries'),
                        'executed_test': matches[0] if len(matches) == 1 else None,
                        'blocker': None if verified else BLOCKERS.get(
                            row['id'], 'Required gate/test did not execute successfully')})
    return results


def release_gates(output):
    return {
        'live-read': cargo('gateway-daemon', '--test', 'github_connector')[:-1] + [
            'live_adapter_profile_github_issue_reaches_authorized_compiled_context_through_shipped_binaries', '--ignored', '--exact', '--test-threads=1'],
        'live-effects': cargo('gateway-daemon', '--test', 'connector_live')[:-1] + ['--ignored', '--test-threads=1'],
        'quality': ['python3', 'tests/scripts/quality-gate.py', '--output', str(output / 'quality')],
    }


def validate_quality(output, sources, revision):
    report = json.loads((output / 'quality/summary.json').read_text())
    expected = json.loads((ROOT / 'tests/scripts/quality-gates.json').read_text())
    if (report.get('status') != 'PASS' or report.get('revision') != revision
            or report.get('source_sha256') != sources
            or [{k: g[k] for k in ('name', 'command')} for g in report['gates']] != expected
            or any(g.get('status') != 'PASS' or type(g.get('exit_code')) is not int
                   or g['exit_code'] != 0 for g in report['gates'])):
        raise ValueError('Full quality manifest did not qualify this candidate')


def run(output, execute, release=False, repository=None):
    output.mkdir(parents=True, exist_ok=False)
    before = fingerprint()
    revision = capture('git', 'rev-parse', 'HEAD')
    before_index = capture('git', 'ls-files', '--stage')
    report = {'schema_version': 1, 'issue': 235, 'parent_issue': 223,
              'system_qualification_issue': 279, 'revision': revision,
              'source_sha256': before, 'status': 'NOT_RUN',
              'git_index': before_index,
              'epic_07_status': 'NOT_ASSESSED' if release else 'NOT_COMPLETE',
              'issue_235_status': 'NOT_COMPLETE', 'closure_allowed': False,
              'closure_issues': [235],
              'evidence_kind': 'synthetic', 'live_evidence': [],
              'worktree_status': capture('git', 'status', '--porcelain'),
              'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'gates': {}, 'limitations': list(BLOCKERS.values())}
    env = dict(os.environ, CARGO_BUILD_JOBS='1', CARGO_TERM_COLOR='never')
    if release:
        if not repository:
            raise ValueError('An explicit isolated live repository is required')
        env.update(CG_CONNECTOR_LIVE_REPOSITORY=repository, CG_CONNECTOR_LIVE_OUTPUT=str(output / 'live'))
    # Token resolution remains in the outer live test adapter; never retained here.
    try:
        manifest = json.loads(MANIFEST.read_text())
        report['compatibility_matrix'] = json.loads(
            (ROOT / 'tests/fixtures/conformance-connector-v1/variations.json').read_text())
        report['toolchain'] = {tool: capture(tool, '--version') for tool in ('rustc', 'cargo', 'python3')}
        commands = {**GATES, **(release_gates(output) if release else {})}
        for name, command in commands.items():
            gate = {'command': command, 'status': 'NOT_RUN', 'passed_tests': []}
            report['gates'][name] = gate
            if not execute:
                continue
            print(f'Connector qualification: {name}', flush=True)
            log = output / (name + '.log')
            gate['log'] = log.name
            try:
                with log.open('w') as stream:
                    result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream,
                                            stderr=subprocess.STDOUT, timeout=28800 if name == 'quality' else 1800)
                gate['exit_code'] = result.returncode
                gate['passed_tests'] = passed_tests(log.read_text())
                success = result.returncode == 0 and (command[0] != 'cargo' or
                          command[1] != 'test' or bool(gate['passed_tests']))
                gate['status'] = 'PASS' if success else 'FAIL'
                if name == 'quality' and success:
                    validate_quality(output, before, revision)
                if name == 'live-read' and success:
                    if len(gate['passed_tests']) != 1:
                        raise ValueError('Exactly one installed live-read scenario is required')
                if name == 'live-effects' and success:
                    live = json.loads((output / 'live/observations.json').read_text())
                    read = json.loads((output / 'live/github-read.json').read_text())
                    if (live.get('evidence_kind') != 'live'
                            or live.get('repository') != repository
                            or [s['mode'] for s in live['scenarios']] != ['ack', 'lost-response']
                            or any(s.get('mutation_replayed') is not False
                                   or s.get('basis') != 'IndependentPostEffectRead'
                                   or s.get('consent_denied_before_dispatch') is not True
                                   or not s.get('branch', '').startswith('cg-qualification-235-')
                                   or not re.fullmatch('[0-9a-f]{40}', s.get('sha', ''))
                                   for s in live['scenarios'])
                            or live.get('cleanup') != 'removed-owned-isolated-refs'
                            or read.get('evidence_kind') != 'live'
                            or read.get('policy_denial_verified') is not True
                            or read.get('selected_evidence_reference') is not True
                            or read.get('shipped_cli_chain') != ['cg-github-mcp', 'cg assess', 'cg plan', 'cg resolve', 'cg compile']):
                        raise ValueError('Missing actual live effect, independent observation or cleanup')
                    report['live_evidence'] = [live, read]
            except (OSError, subprocess.SubprocessError) as error:
                gate.update(status='FAIL', error=type(error).__name__)
        report['requirements'] = reconcile(manifest, report['gates'])
        if (fingerprint() != before or capture('git', 'rev-parse', 'HEAD') != revision
                or capture('git', 'ls-files', '--stage') != before_index):
            raise ValueError('Candidate sources changed during qualification')
        if execute:
            report['status'] = ('QUALIFIED_COMPONENT_SCOPE' if all(
                g['status'] == 'PASS' for g in report['gates'].values()) and all(
                    r['status'] == 'VERIFIED' for r in report['requirements']
                    if r['id'] not in BLOCKERS) else 'FAIL')
            if release and report['status'] == 'QUALIFIED_COMPONENT_SCOPE' and all(
                    r['status'] == 'VERIFIED' for r in report['requirements']):
                report.update(status='QUALIFIED_CONNECTOR_RELEASE', issue_235_status='COMPLETE',
                              closure_allowed=True, evidence_kind='synthetic and live, separately labelled', limitations=[])
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report.update(status='FAIL', error=str(error))
        for row in report.get('requirements', []):
            row.update(status='BLOCKED', blocker='Qualification candidate could not be validated')
    report['finished_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report['artifact_sha256'] = {p.relative_to(output).as_posix(): sha256(p)
                                for p in sorted(output.rglob('*')) if p.is_file()}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path, help='New ignored evidence directory under target/')
    parser.add_argument('--run', action='store_true', help='Execute synthetic checks; default is NOT_RUN inventory')
    parser.add_argument('--release', action='store_true', help='Execute live effects, independent observations and every full quality/coverage gate')
    parser.add_argument('--live-repository', help='Explicit owner/repository for isolated temporary qualification branches')
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(ROOT / 'target'):
        parser.error('Evidence output must be under target/ to keep source bindings stable')
    if args.release and not args.live_repository:
        parser.error('--release requires --live-repository')
    report = run(output, args.run or args.release, args.release, args.live_repository)
    print(f"{report['status']}; EPIC-07 {report['epic_07_status']}: {output / 'report.json'}")
    return 0 if report['status'] in ('QUALIFIED_COMPONENT_SCOPE', 'QUALIFIED_CONNECTOR_RELEASE') else 1


if __name__ == '__main__':
    raise SystemExit(main())
