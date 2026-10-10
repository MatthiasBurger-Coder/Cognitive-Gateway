#!/usr/bin/env python3
"""Reconcile full EPIC-04 acceptance; component success never authorizes closure."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / 'scripts/epic04-acceptance.json'


def load_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def revision():
    return subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()


def contained(base, name):
    path = (base / name).resolve()
    if not path.is_relative_to(base.resolve()):
        raise ValueError('Evidence path escapes its root')
    return path


def validate_binding(report, path, candidate):
    if report.get('revision') != candidate:
        raise ValueError('Evidence revision differs from candidate')
    for field, base in [('source_sha256', ROOT), ('artifact_sha256', path.parent)]:
        hashes = report.get(field)
        if not isinstance(hashes, dict) or not hashes:
            raise ValueError(f'Missing {field}')
        for name, digest in hashes.items():
            if sha256(contained(base, name)) != digest:
                raise ValueError(f'Changed {field}: {name}')
    if report.get('epic_04_status') == 'NOT_COMPLETE':
        raise ValueError('Mandatory evidence explicitly retains EPIC-04 NOT_COMPLETE')
    if report.get('status') in ('NOT_RUN', 'BLOCKED', 'FAIL', 'NOT_COMPLETE'):
        raise ValueError('Mandatory evidence is not successful')


def inspect(kind, path, candidate):
    report = json.loads(path.read_text())
    validate_binding(report, path, candidate)
    if kind in ('component', 'installed') and (
            report.get('epic_04_status') != 'NOT_ASSESSED'
            or report.get('closure_allowed') is not False):
        raise ValueError('Scoped evidence must defer EPIC-04 acceptance and prohibit closure')
    required_sources = {str(p.relative_to(ROOT)) for p in (ROOT / 'crates').rglob('*.rs')}
    required_sources.update(('Cargo.toml', 'Cargo.lock'))
    if not required_sources.issubset(report['source_sha256']):
        raise ValueError('Evidence omits production/test source bindings')
    if kind == 'component':
        component = load_module('epic04_component', 'qualify-codex-local.py')
        if report.get('status') != 'QUALIFIED_COMPONENT_SCOPE':
            raise ValueError('Component gates did not qualify')
        if [(g['name'], g['command']) for g in report['gates']] != component.GATES or any(
                type(g.get('exit_code')) is not int or g['exit_code'] != 0 for g in report['gates']):
            raise ValueError('Required component commands missing or failed')
        component.COVERAGE.check(json.loads((path.parent / 'local-mcp-coverage.json').read_text()))
    elif kind == 'quality':
        gates = json.loads((ROOT / 'scripts/quality-gates.json').read_text())
        if report.get('status') != 'PASS' or [
                {key: gate[key] for key in ('name', 'command')} for gate in report['gates']] != gates:
            raise ValueError('Full quality manifest did not pass')
        if any(g.get('status') != 'PASS' or type(g.get('exit_code')) is not int
               or g['exit_code'] != 0 for g in report['gates']):
            raise ValueError('Quality evidence missing/failed/not run')
        coverage = load_module('epic04_shared_coverage', 'check-session-runtime-coverage.py')
        coverage.GATE.check(json.loads((path.parent / 'shared-sessions/coverage.json').read_text()))
        transitions = path.parent / 'shared-sessions/transitions.jsonl'
        if not transitions.read_text().strip():
            raise ValueError('Missing actual shared-service transitions')
    elif kind == 'installed':
        if report.get('status') != 'QUALIFIED_INSTALLED_SHARED_SESSIONS':
            raise ValueError('Complete installed-client shared-session proof required')
        if not report.get('cli_parity') or not report.get('session_cli_inspection_parity'):
            raise ValueError('Missing canonical/session CLI parity')
        identity = report['installed_client']
        for key in ('launcher_path', 'launcher_sha256', 'version', 'native_binary'):
            if not identity.get(key):
                raise ValueError('Missing installed client identity')
        if sha256(Path(identity['launcher_path'])) != identity['launcher_sha256'] or sha256(
                Path(identity['native_binary']['path'])) != identity['native_binary']['sha256']:
            raise ValueError('Installed client binary changed')
        if not report.get('initialize', {}).get('negotiated_protocol_version'):
            raise ValueError('Missing actual MCP initialize observation')
        if report.get('provider_authentication_used') is not False or report.get('cg_environment') != 'empty':
            raise ValueError('No-key/private host environment not proven')
        if not report.get('cleanup') or any(c.get('forced') is not False or c.get('eof_exit_code') != 0
                                            for c in report['cleanup']):
            raise ValueError('Installed-client EOF cleanup failed')
        required = {'pending_clarification', 'pending_consent', 'completed', 'cancelled',
                    'CG_STALE_REVISION', 'CG_SCOPE_DENIED', 'CG_UNSUPPORTED_CAPABILITY'}
        if not required.issubset(report.get('session_checks', [])):
            raise ValueError('Missing lifecycle/refusal evidence')
    else:
        raise ValueError('Unknown evidence kind')
    if not report.get('binary_sha256') and kind != 'quality':
        raise ValueError('Missing shipped binary hashes')
    if kind != 'quality':
        binaries = report['binary_sha256']
        if set(binaries) != {'cg', 'cg-local', 'cg-mcp'}:
            raise ValueError('All shipped executable identities required')
        for name, digest in binaries.items():
            if sha256(ROOT / 'target/debug' / name) != digest:
                raise ValueError('Shipped binary differs from qualified executable')
    return report


def reconcile(paths):
    candidate = revision()
    manifest = json.loads(MANIFEST.read_text())
    expected = [f'E04-{i:02}' for i in range(1, 25)]
    if [r['id'] for r in manifest['requirements']] != expected:
        raise ValueError('All nineteen original and five added criteria are mandatory, in order')
    evidence, errors = {}, {}
    for kind in ('component', 'quality', 'installed'):
        try:
            evidence[kind] = inspect(kind, paths[kind], candidate)
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors[kind] = str(error)
    # Producers must establish the same binaries; independent rebuilds are not interchangeable.
    if 'component' in evidence and 'installed' in evidence and evidence['component'].get(
            'binary_sha256') != evidence['installed'].get('binary_sha256'):
        errors['installed'] = 'Component and installed-client binary identities differ'
    rows = []
    for requirement in manifest['requirements']:
        blockers = [f'{kind}: {errors[kind]}' for kind in requirement['evidence'] if kind in errors]
        rows.append({**requirement, 'status': 'BLOCKED' if blockers else 'VERIFIED', 'blockers': blockers})
    complete = not errors and all(row['status'] == 'VERIFIED' for row in rows)
    return {'schema_version': 1, 'scope': 'full EPIC-04 #126 including 2026-10-04 additions',
            'revision': candidate, 'status': 'QUALIFIED_EPIC_04' if complete else 'NOT_COMPLETE',
            'epic_04_status': 'COMPLETE' if complete else 'NOT_COMPLETE',
            'closure_allowed': complete, 'closure_issues': [126, 245], 'full_runtime_issue': 279,
            'full_runtime_status': 'SEPARATE_ACCEPTANCE',
            'limitations': list(errors.values()), 'requirements': rows,
            'input_reports': {kind: {'path': str(path), 'sha256': sha256(path) if path.is_file() else None}
                              for kind, path in paths.items()},
            'acceptance_manifest_sha256': sha256(MANIFEST)}


def closure_claims(text):
    # Include comma/and lists and full URLs, regardless of capitalization or verb tense.
    clauses = re.findall(r'\b(?:close[sd]?|fix(?:es|ed)?|resolve[sd]?)\s+([^\n;]+)', text, re.I)
    return {int(number) for clause in clauses for number in re.findall(
        r'(?:#|https://github\.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/)(126|245)\b', clause)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--quality', type=Path)
    parser.add_argument('--component', type=Path)
    parser.add_argument('--installed', type=Path)
    parser.add_argument('--run', action='store_true', help='Execute full quality and installed-client qualification')
    parser.add_argument('--pr-event', type=Path, help='Reject broad closure claims unless acceptance passes')
    args = parser.parse_args()
    claims = None
    if args.pr_event:
        event = json.loads(args.pr_event.read_text())
        pr = event.get('pull_request', {})
        claims = closure_claims((pr.get('title') or '') + '\n' + (pr.get('body') or ''))
        if not claims:
            print('No full EPIC-04 closure claim; component gates retain their own scope.')
            return 0
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    if args.run:
        commands = [
            ('quality', [sys.executable, 'scripts/quality-gate.py', '--output', str(output / 'quality')]),
            ('installed', [sys.executable, 'scripts/cognitive-test-host.py', sys.executable,
                           'scripts/qualify-installed-codex.py', '--shared-session-fixture',
                           '--output', str(output / 'installed')]),
        ]
        for name, command in commands:
            print(f'Full EPIC-04: {name}', flush=True)
            with (output / f'{name}.log').open('w') as log:
                subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=False)
        args.quality = output / 'quality/summary.json'
        args.component = output / 'quality/epic04-component/report.json'
        args.installed = output / 'installed/report.json'
    paths = {kind: getattr(args, kind) or output / f'missing-{kind}.json'
             for kind in ('component', 'quality', 'installed')}
    report = reconcile(paths)
    report['worktree_status'] = subprocess.check_output(
        ['git', 'status', '--porcelain'], cwd=ROOT, text=True)
    report['candidate_source_state'] = 'WORKTREE' if report['worktree_status'] else 'COMMITTED'
    report['requested_closure'] = sorted(claims or [])
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{report['status']}: {output / 'report.json'}")
    return 0 if report['closure_allowed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
