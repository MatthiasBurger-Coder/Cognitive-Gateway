"""Qualification must not confuse successful commands with executed evidence."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('connector_qualification', ROOT / 'tests/scripts/qualify-connectors.py')
Q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(Q)


class QualificationTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads(Q.MANIFEST.read_text())
        self.gates = {name: {'status': 'PASS', 'passed_tests': []} for name in Q.GATES}
        for row in self.manifest['requirements']:
            if row['gate'] in self.gates and row['test']:
                tests = self.gates[row['gate']]['passed_tests']
                if row['test'] not in tests:
                    tests.append(row['test'])

    def test_exact_traceability_and_explicit_system_gaps(self):
        rows = Q.reconcile(self.manifest, self.gates)
        self.assertEqual({r['id'] for r in rows if r['status'] == 'BLOCKED'}, set(Q.BLOCKERS))
        for field in ('missing', 'duplicate', 'reordered'):
            candidate = copy.deepcopy(self.manifest)
            if field == 'missing':
                candidate['requirements'].pop()
            elif field == 'duplicate':
                candidate['requirements'].append(candidate['requirements'][0])
            else:
                candidate['requirements'].reverse()
            with self.assertRaises(ValueError):
                Q.reconcile(candidate, self.gates)

    def test_failed_missing_ignored_and_ambiguous_tests_never_verify(self):
        for change in ('failed', 'missing', 'ignored', 'ambiguous'):
            gates = copy.deepcopy(self.gates)
            g = gates['providers']
            if change == 'failed':
                g['status'] = 'FAIL'
            elif change == 'missing':
                del gates['providers']
            elif change == 'ignored':
                g['passed_tests'] = []
            else:
                g['passed_tests'] *= 2
            row = Q.reconcile(self.manifest, gates)[0]
            self.assertEqual(row['status'], 'BLOCKED', change)

    def test_only_actual_libtest_success_lines_count(self):
        self.assertEqual(Q.passed_tests('test module::yes ... ok\ntest ignored ... ignored\n'
                                      'test failed ... FAILED\nyes: test\nrunning 0 tests\n'), ['module::yes'])

    def fake_process(self, command, **kwargs):
        kwargs['stdout'].write('test actual::test ... ok\n')
        return subprocess.CompletedProcess(command, 0)

    def runner(self, execute=True, process=None, fingerprints=None):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Q, 'fingerprint', side_effect=fingerprints or [{'source': 'digest'}] * 2), \
                patch.object(Q, 'capture', return_value='candidate'), \
                patch.object(Q, 'GATES', {'providers': ['cargo', 'test']}), \
                patch.object(Q.subprocess, 'run', side_effect=process or self.fake_process):
            path = Path(directory) / 'evidence'
            report = Q.run(path, execute)
            self.assertEqual(json.loads((path / 'report.json').read_text()), report)
            self.assertFalse(report['closure_allowed'])
            self.assertEqual(report['live_evidence'], [])
            self.assertEqual(report['epic_07_status'], 'NOT_COMPLETE')
            for name, digest in report['artifact_sha256'].items():
                self.assertEqual(Q.sha256(path / name), digest)
            return report

    def test_dry_run_cannot_pass(self):
        report = self.runner(False)
        self.assertEqual(report['status'], 'NOT_RUN')
        self.assertTrue(all(r['status'] == 'BLOCKED' for r in report['requirements']))

    def test_source_drift_zero_tests_and_failed_commands_retain_failure(self):
        report = self.runner(fingerprints=[{'before': 'a'}, {'after': 'b'}])
        self.assertEqual(report['status'], 'FAIL')
        self.assertIn('sources changed', report['error'])
        self.assertTrue(all(r['status'] == 'BLOCKED' for r in report['requirements']))
        def zero(command, **kwargs):
            return subprocess.CompletedProcess(command, 0)
        def failed(command, **kwargs):
            return subprocess.CompletedProcess(command, 1)
        for process in (zero, failed, OSError(), subprocess.TimeoutExpired('cargo', 1)):
            report = self.runner(process=process)
            self.assertEqual(report['status'], 'FAIL')
            self.assertEqual(report['gates']['providers']['status'], 'FAIL')

    def test_successful_commands_without_required_scenarios_do_not_qualify(self):
        self.assertEqual(self.runner()['status'], 'FAIL')

    def test_component_success_preserves_open_full_acceptance(self):
        # Keep the real gate inventory; each gate gets exactly its required tests.
        def per_gate(command, **kwargs):
            gate = next(name for name, argv in Q.GATES.items() if argv == command)
            for test in sorted(set(r['test'] for r in self.manifest['requirements']
                                   if r['gate'] == gate and r['test'])):
                kwargs['stdout'].write(f'test {test} ... ok\n')
            return subprocess.CompletedProcess(command, 0)
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Q, 'fingerprint', return_value={}), \
                patch.object(Q, 'capture', return_value='candidate'), \
                patch.object(Q.subprocess, 'run', side_effect=per_gate):
            report = Q.run(Path(directory) / 'evidence', True)
            self.assertEqual(report['status'], 'QUALIFIED_COMPONENT_SCOPE')
            self.assertFalse(report['closure_allowed'])

    def test_fingerprint_includes_untracked_sources_and_changes(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(Q, 'ROOT', Path(directory)):
            subprocess.run(['git', 'init', '-q', directory], check=True)
            source = Path(directory) / 'source.rs'
            source.write_text('before')
            self.assertIn('git version', Q.capture('git', '--version'))
            before = Q.fingerprint()
            self.assertEqual(set(before), {'source.rs'})
            source.write_text('after')
            self.assertNotEqual(Q.fingerprint(), before)

    def test_cli_rejects_source_output_and_preserves_dry_run_failure(self):
        with patch.object(sys, 'argv', ['qualify-connectors', '--output', '/tmp/source-evidence']):
            with self.assertRaises(SystemExit) as error:
                Q.main()
            self.assertEqual(error.exception.code, 2)
        for status, expected in [('QUALIFIED_COMPONENT_SCOPE', 0), ('NOT_RUN', 1), ('FAIL', 1)]:
            with patch.object(sys, 'argv', ['qualify-connectors', '--output', str(ROOT / 'target/probe')]), \
                    patch.object(Q, 'run', return_value={'status': status, 'epic_07_status': 'NOT_COMPLETE'}):
                self.assertEqual(Q.main(), expected)

    def test_release_commands_keep_live_execution_explicit_and_select_exact_test(self):
        gates = Q.release_gates(Path('/evidence'))
        for name in ('live-read', 'live-effects'):
            self.assertLess(gates[name].index('--'), gates[name].index('--ignored'))
        self.assertIn('--exact', gates['live-read'])
        self.assertEqual(gates['quality'][-1], '/evidence/quality')
        with patch.object(sys, 'argv', ['qualify-connectors', '--output', str(ROOT / 'target/probe'), '--release']):
            with self.assertRaises(SystemExit):
                Q.main()

    def test_quality_requires_full_manifest_and_exact_sources(self):
        gates = json.loads((ROOT / 'tests/scripts/quality-gates.json').read_text())
        quality = {'status': 'PASS', 'revision': 'candidate', 'source_sha256': {'a': 'b'},
                   'gates': [{**g, 'status': 'PASS', 'exit_code': 0} for g in gates]}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output / 'quality').mkdir()
            path = output / 'quality/summary.json'
            path.write_text(json.dumps(quality))
            Q.validate_quality(output, {'a': 'b'}, 'candidate')
            for change in ('status', 'revision', 'sources', 'missing', 'failed', 'boolean', 'command'):
                bad = copy.deepcopy(quality)
                if change in ('status', 'revision'):
                    bad[change] = 'wrong'
                elif change == 'sources':
                    bad['source_sha256'] = {}
                elif change == 'missing':
                    bad['gates'].pop()
                elif change == 'failed':
                    bad['gates'][0]['status'] = 'NOT_RUN'
                elif change == 'boolean':
                    bad['gates'][0]['exit_code'] = False
                else:
                    bad['gates'][0]['command'] = 'true'
                path.write_text(json.dumps(bad))
                with self.assertRaises(ValueError):
                    Q.validate_quality(output, {'a': 'b'}, 'candidate')

    def release_report(self, change=None):
        def execute(command, **kwargs):
            gate = next(name for name, argv in {**Q.GATES, **Q.release_gates(output)}.items() if argv == command)
            tests = {r['test'] for r in self.manifest['requirements'] if r['gate'] == gate and r['test']}
            if gate == 'live-read':
                tests = {'live_adapter_profile_github_issue_reaches_authorized_compiled_context_through_shipped_binaries'}
                if change == 'extra-read':
                    tests.add('foreign_test')
            for test in sorted(tests):
                kwargs['stdout'].write(f'test {test} ... ok\n')
            if gate == 'live-effects':
                live = output / 'live'
                live.mkdir()
                modes = ['ack', 'lost-response'] if change != 'missing-effect' else ['ack']
                (live / 'observations.json').write_text(json.dumps({'evidence_kind': 'live',
                    'repository': 'owner/isolated-repository',
                    'cleanup': 'removed-owned-isolated-refs',
                    'scenarios': [{'mode': mode, 'mutation_replayed': False,
                        'basis': 'IndependentPostEffectRead', 'consent_denied_before_dispatch': True,
                        'branch': 'cg-qualification-235-unit', 'sha': 'a' * 40} for mode in modes]}))
                (live / 'github-read.json').write_text(json.dumps({'evidence_kind': 'live',
                    'policy_denial_verified': True, 'selected_evidence_reference': True,
                    'shipped_cli_chain': ['cg-github-mcp', 'cg assess', 'cg plan', 'cg resolve', 'cg compile']}))
            if gate == 'quality':
                (output / 'quality').mkdir()
                manifest = json.loads((ROOT / 'tests/scripts/quality-gates.json').read_text())
                (output / 'quality/summary.json').write_text(json.dumps({'status': 'PASS',
                    'revision': 'candidate', 'source_sha256': {},
                    'gates': [{**g, 'status': 'PASS', 'exit_code': 0} for g in manifest]}))
            return subprocess.CompletedProcess(command, 0)
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Q, 'fingerprint', return_value={}), \
                patch.object(Q, 'capture', return_value='candidate'), \
                patch.object(Q.subprocess, 'run', side_effect=execute):
            output = Path(directory) / 'evidence'
            return Q.run(output, True, True, 'owner/isolated-repository')

    def test_release_only_qualifies_after_live_and_full_quality(self):
        report = self.release_report()
        self.assertEqual(report['status'], 'QUALIFIED_CONNECTOR_RELEASE')
        self.assertEqual(report['issue_235_status'], 'COMPLETE')
        self.assertTrue(report['closure_allowed'])
        self.assertEqual(report['closure_issues'], [235])
        self.assertEqual(report['epic_07_status'], 'NOT_ASSESSED')
        self.assertTrue(all(r['status'] == 'VERIFIED' for r in report['requirements']))
        for change in ('extra-read', 'missing-effect'):
            report = self.release_report(change)
            self.assertEqual(report['status'], 'FAIL')
            self.assertFalse(report['closure_allowed'])

    def test_release_cli_and_missing_repository(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Q, 'fingerprint', return_value={}), \
                patch.object(Q, 'capture', return_value='candidate'):
            with self.assertRaisesRegex(ValueError, 'explicit isolated'):
                Q.run(Path(directory) / 'evidence', True, True)
        with patch.object(sys, 'argv', ['qualify-connectors', '--output', str(ROOT / 'target/probe'),
                                      '--release', '--live-repository', 'owner/repo']), \
                patch.object(Q, 'run', return_value={'status': 'QUALIFIED_CONNECTOR_RELEASE', 'epic_07_status': 'NOT_ASSESSED'}):
            self.assertEqual(Q.main(), 0)

    def test_directly_executed_conformance_peer_is_executable_in_git(self):
        mode = subprocess.check_output(['git', 'ls-files', '--stage', '--',
            'tests/fixtures/conformance-connector-v1/github-peer.py'], cwd=ROOT, text=True).split()[0]
        self.assertEqual(mode, '100755', 'Windows mount permissions cannot substitute for the Git execute bit')

    def test_git_index_mode_drift_invalidates_qualification(self):
        indices = iter(['before-mode', 'changed-mode'])
        def capture(*command):
            return next(indices) if command == ('git', 'ls-files', '--stage') else 'candidate'
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(Q, 'fingerprint', return_value={}), patch.object(Q, 'capture', side_effect=capture):
            report = Q.run(Path(directory) / 'evidence', False)
            self.assertEqual(report['status'], 'FAIL')
            self.assertFalse(report['closure_allowed'])

    def test_manual_coverage_launchers_clean_the_actual_measurement_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            cargo = folder / 'cargo'
            capture = folder / 'environment.json'
            cargo.write_text('#!' + sys.executable + '\n' +
                'import json,os,sys\n' +
                'open(os.environ["CAPTURE"],"w").write(json.dumps({"args":sys.argv[1:],'
                '"target":os.environ.get("CARGO_TARGET_DIR"),'
                '"coverage":os.environ.get("CARGO_LLVM_COV_TARGET_DIR")}))\n' +
                'sys.exit(72)\n')
            cargo.chmod(0o755)
            environment = {**os.environ, 'PATH': str(folder) + os.pathsep + os.environ['PATH'],
                           'CAPTURE': str(capture), 'CG_COGNITIVE_TEST_DATABASE': 'test-only-not-connected'}
            for key in ('CARGO_TARGET_DIR', 'CARGO_LLVM_COV_TARGET_DIR'):
                environment.pop(key, None)
            for script, target in [('qualify-shared-sessions.sh', 'session'),
                                   ('qualify-before-journal.sh', 'before-journal'),
                                   ('qualify-atomic-before.sh', 'atomic-before')]:
                result = subprocess.run(['bash', str(ROOT / 'tests/scripts' / script),
                    str(folder / target)], env=environment, capture_output=True)
                self.assertEqual(result.returncode, 72, result.stderr)
                invoked = json.loads(capture.read_text())
                self.assertEqual(invoked['args'], ['llvm-cov', 'clean', '--workspace'])
                self.assertEqual(invoked['target'], str(ROOT / 'target' / (target + '-coverage')))
                self.assertEqual(invoked['coverage'], invoked['target'])


if __name__ == '__main__':
    unittest.main()
