#!/usr/bin/env python3
"""Retain EPIC-04 inbound component evidence, separately from #279 runtime release."""
import argparse
import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('coverage_gate', ROOT / 'scripts/check-local-mcp-coverage.py')
COVERAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COVERAGE)

GATES = [
    ('build', ['cargo', 'build', '-p', 'gateway-daemon', '--bin', 'cg', '--bin', 'cg-mcp', '--bin', 'cg-local', '--locked']),
    ('canonical-security', ['cargo', 'test', '-p', 'gateway-application', '--test', 'codex_facade', '--locked']),
    ('bridge-isolation-faults', ['cargo', 'test', '-p', 'gateway-daemon', '--lib', '--test', 'local_mcp',
                                 '--test', 'codex_isolation', '--test', 'codex_local_cli',
                                 '--test', 'codex_canonical', '--test', 'declarative_cli', '--test', 'codex_qualification', '--locked']),
    ('executable-goldens', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/codex-local', '-v']),
    ('protocol-schemas', ['python3', 'scripts/check-local-mcp-protocol.py']),
    ('contract-goldens', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/contracts', '-v']),
    ('architecture', ['bash', 'scripts/check-architecture.sh']),
    ('architecture-regressions', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/architecture', '-v']),
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('clippy', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fingerprint():
    paths = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).decode().split('\0')
    return {p: digest(ROOT / p) for p in sorted(set(paths)) if p and (ROOT / p).is_file()}


def validate(report):
    if (report['source_sha256'] != fingerprint()
            or report['revision'] != subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()):
        raise ValueError('Candidate sources changed during qualification')
    if ([(g['name'], g['command']) for g in report['gates']] != GATES
            or any(g.get('exit_code') != 0 for g in report['gates'])):
        raise ValueError('Required component gate missing or failed')
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    parser.add_argument('--coverage-report', type=Path, help='Current release gate coverage; otherwise measure now')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {'schema_version': 1, 'scope': 'EPIC-04 inbound no-key component', 'status': 'RUNNING',
              'epic_04_status': 'NOT_COMPLETE', 'full_runtime_issue': 279,
              'client': {'kind': 'synthetic protocol-conformant fixture', 'name': 'codex', 'version': '1.0'},
              'protocol_version': '2025-11-25', 'application_schema_version': '1.0',
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'worktree_status': subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True),
              'source_sha256': fingerprint(), 'gates': [],
              'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'limitations': [
                  'No installed Codex client/account or provider authentication is qualified.',
                  'This runner qualifies canonical and inbound component behavior; shared-service lifecycle requires its separate live gate.',
                  'Injected session projections are component proof; delivered shared durable services require PostgreSQL and installed-client evidence.',
                  'No connector/model completion, durable session lifecycle, #279 runtime or whole release gate is claimed.'],
              'requirement_matrix': 'docs/codex-release-qualification.md'}
    try:
        report['toolchain'] = {name: subprocess.check_output(command, cwd=ROOT, text=True).strip()
                               for name, command in [('rustc', ['rustc', '--version']),
                                                     ('cargo', ['cargo', '--version']),
                                                     ('coverage', ['cargo', 'llvm-cov', '--version']),
                                                     ('python', ['python3', '--version'])]}
        for name, command in GATES:
            print(f'EPIC-04 component: {name}', flush=True)
            log = output / (name + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, timeout=900)
            report['gates'].append({'name': name, 'command': command, 'exit_code': result.returncode})
            if result.returncode:
                raise ValueError(f'{name} failed; inspect {log.name}')
        coverage = output / 'local-mcp-coverage.json'
        if args.coverage_report:
            coverage.write_bytes(args.coverage_report.read_bytes())
        else:
            command = ['cargo', 'llvm-cov', '-p', 'gateway-application', '-p', 'gateway-daemon', '--lib',
                       '--bin', 'cg', '--bin', 'cg-mcp', '--bin', 'cg-local', '--test', 'local_mcp', '--test', 'codex_facade',
                       '--test', 'codex_isolation', '--test', 'codex_local_cli', '--test', 'codex_qualification',
                       '--test', 'codex_canonical', '--test', 'declarative_cli', '--locked', '--json', '--output-path', str(coverage)]
            with (output / 'coverage.log').open('w') as stream:
                subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, timeout=900, check=True)
        report['binary_sha256'] = {name: digest(ROOT / 'target/debug' / name) for name in ('cg', 'cg-mcp', 'cg-local')}
        report['coverage_source'] = 'supplied existing measurement' if args.coverage_report else 'measured by this run'
        report['coverage'] = COVERAGE.check(json.loads(coverage.read_text()))
        validate(report)
        report['status'] = 'QUALIFIED_COMPONENT_SCOPE'
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report['status'] = 'FAIL'
        report['error'] = str(error)
    finally:
        report['finished_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        report['artifact_sha256'] = {p.name: digest(p) for p in sorted(output.iterdir()) if p.is_file()}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{report['status']}: {output / 'report.json'}", flush=True)
    return 0 if report['status'] == 'QUALIFIED_COMPONENT_SCOPE' else 1


if __name__ == '__main__':
    sys.exit(main())
