"""Full acceptance and closure claims must reject partial or contradictory evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('epic04_acceptance', ROOT / 'scripts/qualify-epic04.py')
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class Epic04AcceptanceTests(unittest.TestCase):
    def test_parent_matrix_preserves_all_original_and_added_criteria(self):
        rows = json.loads(GATE.MANIFEST.read_text())['requirements']
        self.assertEqual([r['id'] for r in rows], [f'E04-{i:02}' for i in range(1, 25)])
        for row in rows:
            for key in ('source', 'production_path', 'dependencies', 'commands', 'required_evidence_levels'):
                self.assertTrue(row[key])
            self.assertTrue((ROOT / row['executable_tests']).is_file(), row['id'])
        self.assertTrue(all('installed' in row['evidence'] and 'quality' in row['evidence'] for row in rows[19:]))

    def test_missing_evidence_cannot_authorize_closure(self):
        with tempfile.TemporaryDirectory() as temp:
            result = GATE.reconcile({kind: Path(temp) / kind for kind in ('component', 'quality', 'installed')})
            self.assertEqual(result['status'], 'NOT_COMPLETE')
            self.assertFalse(result['closure_allowed'])
            self.assertEqual(len(result['requirements']), 24)
            self.assertTrue(all(r['status'] == 'BLOCKED' for r in result['requirements']))
            self.assertEqual(result['full_runtime_issue'], 279)

    def test_complete_gate_only_and_each_partial_refuses_closure(self):
        reports = {kind: {'binary_sha256': {'cg': 'same'}} for kind in ('component', 'quality', 'installed')}
        paths = {kind: ROOT / 'Cargo.toml' for kind in reports}
        with patch.object(GATE, 'inspect', side_effect=lambda kind, *args: reports[kind]):
            result = GATE.reconcile(paths)
            self.assertTrue(result['closure_allowed'])
            self.assertEqual(result['status'], 'QUALIFIED_EPIC_04')
        for missing in reports:
            def inspect(kind, *args):
                if kind == missing:
                    raise ValueError('NOT_RUN')
                return reports[kind]
            with self.subTest(missing=missing), patch.object(GATE, 'inspect', side_effect=inspect):
                self.assertFalse(GATE.reconcile(paths)['closure_allowed'])
        reports['installed']['binary_sha256'] = {'cg': 'different'}
        with patch.object(GATE, 'inspect', side_effect=lambda kind, *args: reports[kind]):
            self.assertFalse(GATE.reconcile(paths)['closure_allowed'])

    def test_binding_rejects_incomplete_changed_missing_and_escape(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source'
            artifact = root / 'transcript'
            source.write_text('candidate')
            artifact.write_text('observed')
            good = {'revision': 'candidate', 'status': 'PASS',
                    'source_sha256': {'source': GATE.sha256(source)},
                    'artifact_sha256': {'transcript': GATE.sha256(artifact)}}
            with patch.object(GATE, 'ROOT', root):
                GATE.validate_binding(good, root / 'report.json', 'candidate')
                for field, value in [('revision', 'old'), ('source_sha256', {}), ('artifact_sha256', {}),
                                     ('status', 'NOT_RUN'), ('status', 'BLOCKED'), ('status', 'FAIL'),
                                     ('epic_04_status', 'NOT_COMPLETE'),
                                     ('source_sha256', {'../escape': 'digest'})]:
                    bad = copy.deepcopy(good)
                    bad[field] = value
                    with self.subTest(field=field, value=value), self.assertRaises((OSError, ValueError)):
                        GATE.validate_binding(bad, root / 'report.json', 'candidate')
                artifact.write_text('tampered')
                with self.assertRaises(ValueError):
                    GATE.validate_binding(good, root / 'report.json', 'candidate')
                artifact.write_text('observed')
                source.unlink()
                with self.assertRaises(OSError):
                    GATE.validate_binding(good, root / 'report.json', 'candidate')

    def test_installed_client_full_lifecycle_and_failure_boundaries(self):
        report = json.loads((ROOT / 'docs/evidence/EPIC-04.14-installed-codex/report.json').read_text())
        report['binary_sha256'] = {name: 'digest' for name in ('cg', 'cg-mcp', 'cg-local')}
        report['installed_client']['launcher_sha256'] = 'digest'
        report['installed_client']['native_binary']['sha256'] = 'digest'
        report['source_sha256'].update({str(p.relative_to(ROOT)): 'digest' for p in (ROOT / 'crates').rglob('*.rs')})
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'report.json'
            def inspect(value):
                path.write_text(json.dumps(value))
                with patch.object(GATE, 'validate_binding'), patch.object(GATE, 'sha256', return_value='digest'):
                    return GATE.inspect('installed', path, 'candidate')
            self.assertEqual(inspect(report)['status'], 'QUALIFIED_INSTALLED_SHARED_SESSIONS')
            changes = [('status', 'QUALIFIED_INSTALLED_INSPECTION'), ('status', 'QUALIFIED_INSTALLED_CANONICAL'),
                       ('session_checks', ['completed']), ('cli_parity', False),
                       ('session_cli_inspection_parity', False), ('installed_client', {}),
                       ('initialize', {}), ('provider_authentication_used', True),
                       ('cg_environment', 'inherited'), ('cleanup', []),
                       ('cleanup', [{'forced': True, 'eof_exit_code': 0}]),
                       ('cleanup', [{'forced': False, 'eof_exit_code': 1}]),
                       ('binary_sha256', {'cg': 'digest'}), ('source_sha256', {})]
            for key, value in changes:
                bad = copy.deepcopy(report)
                bad[key] = value
                with self.subTest(key=key, value=value), self.assertRaises((ValueError, KeyError)):
                    inspect(bad)

    def test_real_installed_report_cannot_be_upgraded_to_full_parent(self):
        report = json.loads((ROOT / 'docs/evidence/EPIC-04.14-installed-codex/report.json').read_text())
        with self.assertRaises(ValueError):
            GATE.validate_binding(report, ROOT / 'docs/evidence/EPIC-04.14-installed-codex/report.json', GATE.revision())

    def test_closure_claims_include_lists_urls_and_capitalization(self):
        for text in ('Closes #126', 'FIXES #245', 'Resolves #292, #126 and #245',
                     'Closed https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/126'):
            self.assertTrue(GATE.closure_claims(text), text)
        for text in ('Refs #126', 'Closes #296', 'Closes #1260', '#245 remains component-only'):
            self.assertFalse(GATE.closure_claims(text), text)

    def test_cli_missing_reports_nonzero_and_nonclosure_pr_passes(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / 'out'
            result = subprocess.run([sys.executable, GATE.__file__, '--output', str(output)], capture_output=True)
            self.assertEqual(result.returncode, 1)
            self.assertFalse(json.loads((output / 'report.json').read_text())['closure_allowed'])
            event = Path(temp) / 'event.json'
            event.write_text(json.dumps({'pull_request': {'title': 'Closes #296', 'body': 'Refs #126'}}))
            result = subprocess.run([sys.executable, GATE.__file__, '--output', str(output), '--pr-event', str(event)], capture_output=True)
            self.assertEqual(result.returncode, 0)
