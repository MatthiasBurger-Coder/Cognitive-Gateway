"""Exercise fail-closed qualification orchestration without a live provider."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('invocation_qualifier', ROOT / 'scripts/qualify-invocation.py')
QUALIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUALIFY)


class InvocationQualificationTests(unittest.TestCase):
    def manifest(self):
        test = 'crates/gateway-application/tests/invocation_qualification.rs::replay_rechecks_current_authority_and_returns_no_dispatchable_result'
        return {'schema_version': '1.0', 'gates': [
            {'id': 'first', 'command': 'first', 'rust_tests': True},
            {'id': 'second', 'command': 'second', 'rust_tests': False}],
            'criteria': [{'id': 'R1', 'requirement': 'Replay', 'tests': [test], 'gates': ['first', 'second']}]}

    def test_all_parent_child_and_budget_criteria_are_mapped(self):
        manifest = QUALIFY.validate_manifest(json.loads(QUALIFY.MANIFEST.read_text()))
        ids = {c['id'] for c in manifest['criteria']}
        self.assertTrue({f'P{i:02}' for i in range(1, 23)} <= ids)
        self.assertTrue({f'Q{i:02}' for i in range(1, 12)} <= ids)
        self.assertTrue({f'T{i:02}' for i in range(1, 5)} <= ids)
        source = (ROOT / 'crates/gateway-application/src/invocation_qualification.rs').read_text()
        for forbidden in ['InvocationAttemptPort', 'PromptInvocationPort', 'std::fs', 'std::net', 'VerifiedOutput,']:
            self.assertNotIn(forbidden, source)

    def test_invalid_manifests_cannot_omit_proof(self):
        for edit in [lambda m: m.update(schema_version='2.0'),
                     lambda m: m.update(gates=[]),
                     lambda m: m['gates'].append(m['gates'][0]),
                     lambda m: m['gates'][0].update(command=''),
                     lambda m: m.update(criteria=[]),
                     lambda m: m['criteria'].append(m['criteria'][0]),
                     lambda m: m['criteria'][0].update(gates=['absent']),
                     lambda m: m['criteria'][0].update(tests=['crates/gateway-application/tests/invocation_qualification.rs::absent'])]:
            manifest = self.manifest()
            edit(manifest)
            with self.assertRaises(ValueError):
                QUALIFY.validate_manifest(manifest)

    def test_plan_failure_zero_tests_exception_and_source_drift_block(self):
        for mode in ['plan', 'failed', 'zero', 'error', 'drift', 'missing', 'pass']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                calls, snapshots = [], []
                def snapshot(_):
                    snapshots.append(1)
                    return {'source': 'changed' if mode == 'drift' and len(snapshots) > 1 else 'original'}
                def executor(command, log, env):
                    calls.append(command)
                    self.assertIn('EVIDENCE_DIR', env)
                    if mode == 'error':
                        raise OSError('unavailable')
                    log.write_text(('test replay_rechecks_current_authority_and_returns_no_dispatchable_result ... ok\n' if mode != 'missing' else '') + ('test result: ok. 0 passed;\n' if mode == 'zero' else 'test result: ok. 1 passed;\n'))
                    return 1 if mode == 'failed' else 0
                report = QUALIFY.run(Path(temp) / 'evidence', mode != 'plan', self.manifest(), executor, snapshot)
                self.assertEqual(report['release_status'], 'NOT_COMPLETE')
                self.assertEqual(report['model_promotion'], 'NOT_RUN')
                self.assertEqual(report['status'], 'COMPONENT_PASS' if mode == 'pass' else 'NOT_RUN' if mode == 'plan' else 'FAIL')
                self.assertEqual(len(calls), 0 if mode == 'plan' else 2 if mode in ['pass', 'drift', 'missing'] else 1)
                self.assertEqual(json.loads((Path(temp) / 'evidence/qualification.json').read_text()), report)
                if mode == 'drift':
                    self.assertEqual(report['criteria'][0]['status'], 'NOT_VERIFIED')
                if mode in ['failed', 'zero']:
                    self.assertEqual(report['gates'][1]['status'], 'NOT_RUN')
                    self.assertEqual(report['criteria'][0]['status'], 'NOT_VERIFIED')

    def test_output_cannot_exclude_source_from_binding(self):
        with self.assertRaises(ValueError):
            QUALIFY.run(ROOT / 'crates/evidence', False, self.manifest())

    def test_parent_requires_local_bridge_tests_and_coverage(self):
        manifest = QUALIFY.validate_manifest(json.loads(QUALIFY.MANIFEST.read_text()))
        gate = next(g for g in manifest['gates'] if g['id'] == 'local-model-python')
        self.assertIn('-m unittest discover -s tests/local-model', gate['command'])
        self.assertIn('--source=services/local-model', gate['command'])
        self.assertIn('--include="*/canonical.py" --fail-under=95', gate['command'])
        self.assertIn('--include="*/service.py" --fail-under=95', gate['command'])
        for criterion in manifest['criteria']:
            if criterion['id'] == 'P17' or any('/model_adapters.rs::' in t for t in criterion['tests']):
                self.assertIn(gate['id'], criterion['gates'], criterion['id'])

    def test_real_bridge_test_or_coverage_failure_blocks_parent_qualification(self):
        from unittest.mock import patch
        manifest = json.loads(QUALIFY.MANIFEST.read_text())
        bridge = next(g for g in manifest['gates'] if g['id'] == 'local-model-python')
        # Execute the actual bridge gate against a tiny isolated bridge. Other
        # gates are synthetic so this regression never runs Rust or live models.
        for mode in ['test-failure', 'low-coverage', 'pass']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                source = root / 'services/local-model/canonical.py'
                source.parent.mkdir(parents=True)
                source.write_text("def classify(value):\n    if value:\n        return 'yes'\n    return 'no'\n")
                (source.parent / 'service.py').write_text('def ready():\n    return True\n')
                tests = root / 'tests/local-model'
                tests.mkdir(parents=True)
                assertions = ("self.assertEqual(classify(True), 'no')" if mode == 'test-failure'
                              else "self.assertEqual(classify(True), 'yes')")
                if mode == 'pass':
                    assertions += "\n        self.assertEqual(classify(False), 'no')"
                (tests / 'test_bridge.py').write_text(
                    "import sys, unittest\nfrom pathlib import Path\n"
                    "sys.path.insert(0, str(Path('services/local-model').resolve()))\n"
                    "from canonical import classify\nfrom service import ready\n"
                    "class BridgeTests(unittest.TestCase):\n"
                    "    def test_result(self):\n        self.assertTrue(ready())\n        " + assertions + "\n")
                def executor(command, log, env):
                    if command == bridge['command']:
                        with patch.object(QUALIFY, 'ROOT', root):
                            return QUALIFY.execute(command, log, env)
                    log.write_text('\n'.join(
                        f"test {test.split('::', 1)[1]} ... ok"
                        for c in manifest['criteria'] for test in c['tests'])
                        + '\ntest result: ok. 1 passed;\n')
                    return 0
                report = QUALIFY.run(root / 'evidence', True, manifest, executor,
                                     snapshot=lambda _: {'fixture': 'unchanged'})
                gates = {g['id']: g for g in report['gates']}
                self.assertEqual(gates[bridge['id']]['status'], 'PASS' if mode == 'pass' else 'FAIL')
                self.assertEqual(report['status'], 'COMPONENT_PASS' if mode == 'pass' else 'FAIL')
                self.assertEqual(report['release_status'], 'NOT_COMPLETE')
                if mode != 'pass':
                    self.assertEqual(gates['architecture']['status'], 'NOT_RUN')
                    self.assertTrue(all(c['status'] == 'NOT_VERIFIED' for c in report['criteria']
                                        if bridge['id'] in c['gates']))
                if mode == 'test-failure':
                    self.assertFalse((root / 'evidence/local-model-python.json').exists())
                else:
                    coverage = json.loads((root / 'evidence/local-model-python.json').read_text())
                    self.assertEqual(coverage['files']['services/local-model/canonical.py']['summary']['percent_covered'], 100 if mode == 'pass' else 75)

class RunnerInfrastructureTests(unittest.TestCase):
    def test_real_commands_keep_logs_and_propagate_pipeline_failure(self):
        import os
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / 'command.log'
            self.assertEqual(QUALIFY.execute('printf evidence', log, os.environ.copy()), 0)
            self.assertEqual(log.read_text(), 'evidence')
            self.assertNotEqual(QUALIFY.execute('false | cat', log, os.environ.copy()), 0)

    def test_actual_git_fingerprint_covers_untracked_files_and_detects_changes(self):
        import subprocess
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            source = root / 'source.rs'
            source.write_text('original')
            historical = root / 'docs/evidence/old.json'
            historical.parent.mkdir(parents=True)
            historical.write_text('historical')
            output = root / 'target/evidence'
            output.mkdir(parents=True)
            (output / 'qualification.json').write_text('generated')
            with patch.object(QUALIFY, 'ROOT', root):
                before = QUALIFY.fingerprint(output)
                self.assertEqual(set(before), {'source.rs'})
                source.write_text('changed')
                self.assertNotEqual(before, QUALIFY.fingerprint(output))

    def test_cli_status_and_required_output(self):
        from unittest.mock import patch
        for status, expected in [('COMPONENT_PASS', 0), ('FAIL', 1), ('NOT_RUN', 1)]:
            with patch('sys.argv', ['qualify-invocation', '--output', '/tmp/unused-cg270']), patch.object(QUALIFY, 'run', return_value={'status': status, 'release_status': 'NOT_COMPLETE'}) as run:
                self.assertEqual(QUALIFY.main(), expected)
                self.assertFalse(run.call_args.args[1])
        with patch('sys.argv', ['qualify-invocation']), self.assertRaises(SystemExit):
            QUALIFY.main()


if __name__ == '__main__':
    unittest.main()
