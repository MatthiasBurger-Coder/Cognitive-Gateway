"""Full acceptance and closure claims must reject partial or contradictory evidence."""
import copy
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import subprocess
import shutil
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

    @contextmanager
    def evidence_fixture(self):
        """Validate the adjudicator with disk evidence; this is no runtime qualification."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            scripts = root / 'scripts'
            scripts.mkdir()
            for name in ('qualify-codex-local.py', 'check-local-mcp-coverage.py',
                         'check-session-runtime-coverage.py', 'quality-gates.json',
                         'epic04-acceptance.json'):
                shutil.copyfile(ROOT / 'scripts' / name, scripts / name)
            for name in ('Cargo.toml', 'Cargo.lock', 'crates/example/src/lib.rs'):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('fixture source: ' + name)
            binary_hashes = {}
            for name in ('cg', 'cg-local', 'cg-mcp'):
                path = root / 'target/debug' / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('fixture binary: ' + name)
                binary_hashes[name] = GATE.sha256(path)
            launcher, native = root / 'codex-launcher', root / 'codex-native'
            launcher.write_text('fixture launcher')
            native.write_text('fixture native client')
            sources = {str(p.relative_to(root)): GATE.sha256(p)
                       for p in root.rglob('*') if p.is_file() and 'target' not in p.parts
                       and p not in (launcher, native)}
            paths, reports = {}, {}
            with patch.object(GATE, 'ROOT', root), \
                 patch.object(GATE, 'MANIFEST', scripts / 'epic04-acceptance.json'), \
                 patch.object(GATE, 'revision', return_value='candidate'):
                component = GATE.load_module('fixture_component', 'qualify-codex-local.py')
                shared = GATE.load_module('fixture_shared', 'check-session-runtime-coverage.py')
                for kind in ('component', 'quality', 'installed'):
                    directory = root / 'evidence' / kind
                    directory.mkdir(parents=True)
                    paths[kind] = directory / 'report.json'
                    (directory / 'observed.log').write_text('fixture observation')
                    report = {'revision': 'candidate', 'source_sha256': copy.deepcopy(sources)}
                    if kind == 'quality':
                        report.update(status='PASS', gates=[{**gate, 'status': 'PASS', 'exit_code': 0}
                            for gate in json.loads((scripts / 'quality-gates.json').read_text())])
                    else:
                        report.update(epic_04_status='NOT_ASSESSED', closure_allowed=False,
                                      binary_sha256=copy.deepcopy(binary_hashes))
                    if kind == 'component':
                        report.update(status='QUALIFIED_COMPONENT_SCOPE', gates=[
                            {'name': name, 'command': command, 'exit_code': 0}
                            for name, command in component.GATES])
                    elif kind == 'installed':
                        report.update(status='QUALIFIED_INSTALLED_SHARED_SESSIONS', cli_parity=True,
                            session_cli_inspection_parity=True, provider_authentication_used=False,
                            cg_environment='empty', cleanup=[{'forced': False, 'eof_exit_code': 0}],
                            initialize={'negotiated_protocol_version': '2025-11-25'},
                            installed_client={'launcher_path': str(launcher),
                                'launcher_sha256': GATE.sha256(launcher), 'version': 'fixture',
                                'native_binary': {'path': str(native), 'sha256': GATE.sha256(native)}},
                            session_checks=['pending_clarification', 'pending_consent', 'completed',
                                'cancelled', 'CG_STALE_REVISION', 'CG_SCOPE_DENIED',
                                'CG_UNSUPPORTED_CAPABILITY'])
                    if kind in ('component', 'quality'):
                        coverage_path = directory / ('local-mcp-coverage.json' if kind == 'component'
                                                     else 'shared-sessions/coverage.json')
                        coverage_path.parent.mkdir(parents=True, exist_ok=True)
                        expected = component.COVERAGE.EXPECTED if kind == 'component' else shared.GATE.EXPECTED
                        coverage_path.write_text(json.dumps({'data': [{'files': [
                            {'filename': name, 'summary': {'lines': {'count': 100, 'covered': 95}}}
                            for name in expected]}]}))
                    if kind == 'quality':
                        (directory / 'shared-sessions/transitions.jsonl').write_text('{"state":"completed"}\n')
                    report['artifact_sha256'] = {str(p.relative_to(directory)): GATE.sha256(p)
                        for p in directory.rglob('*') if p.is_file()}
                    reports[kind] = report
                    paths[kind].write_text(json.dumps(report))
                yield root, paths, reports

    def test_real_inspection_and_reconciliation_accept_complete_scoped_evidence(self):
        with self.evidence_fixture() as (_, paths, reports):
            for kind in paths:
                self.assertEqual(GATE.inspect(kind, paths[kind], 'candidate'), reports[kind])
            result = GATE.reconcile(paths)
            self.assertTrue(result['closure_allowed'])
            self.assertEqual(result['status'], 'QUALIFIED_EPIC_04')
            self.assertEqual(result['epic_04_status'], 'COMPLETE')
            self.assertEqual(len(result['requirements']), 24)
            self.assertTrue(all(row['status'] == 'VERIFIED' for row in result['requirements']))
            self.assertTrue(all(not reports[k]['closure_allowed'] for k in ('component', 'installed')))

    def test_real_reconciliation_rejects_missing_stale_failed_and_contradictory_inputs(self):
        for kind in ('component', 'quality', 'installed'):
            for change in ('missing', 'stale', 'failed', 'explicit_incomplete', 'source_omitted',
                           'artifact_omitted', 'gate_missing', 'gate_failed'):
                if kind == 'installed' and change.startswith('gate_'):
                    continue
                with self.subTest(kind=kind, change=change), self.evidence_fixture() as (_, paths, reports):
                    report = reports[kind]
                    if change == 'missing':
                        paths[kind].unlink()
                    else:
                        if change == 'stale':
                            report['revision'] = 'old'
                        elif change == 'failed':
                            report['status'] = 'FAIL'
                        elif change == 'explicit_incomplete':
                            report['epic_04_status'] = 'NOT_COMPLETE'
                        elif change == 'source_omitted':
                            report['source_sha256'].pop('crates/example/src/lib.rs')
                        elif change == 'artifact_omitted':
                            report['artifact_sha256'] = {}
                        elif change == 'gate_missing':
                            report['gates'].pop()
                        elif change == 'gate_failed':
                            report['gates'][0]['exit_code'] = 1
                        paths[kind].write_text(json.dumps(report))
                    self.assertFalse(GATE.reconcile(paths)['closure_allowed'])

    def test_scoped_inputs_cannot_assess_or_authorize_parent_completion(self):
        for kind in ('component', 'installed'):
            for field, value in [('epic_04_status', None), ('epic_04_status', 'UNKNOWN'),
                                 ('epic_04_status', 'COMPLETE'), ('closure_allowed', None),
                                 ('closure_allowed', True), ('closure_allowed', 0)]:
                with self.subTest(kind=kind, field=field, value=value), self.evidence_fixture() as (_, paths, reports):
                    if value is None:
                        reports[kind].pop(field)
                    else:
                        reports[kind][field] = value
                    paths[kind].write_text(json.dumps(reports[kind]))
                    self.assertFalse(GATE.reconcile(paths)['closure_allowed'])

    def test_real_reconciliation_rejects_source_artifact_and_executable_drift(self):
        for name in ('Cargo.toml', 'crates/example/src/lib.rs',
                     'evidence/component/observed.log', 'evidence/quality/shared-sessions/coverage.json',
                     'evidence/quality/shared-sessions/transitions.jsonl', 'evidence/installed/observed.log',
                     'target/debug/cg', 'target/debug/cg-local', 'target/debug/cg-mcp',
                     'codex-launcher', 'codex-native'):
            with self.subTest(name=name), self.evidence_fixture() as (root, paths, _):
                (root / name).write_text('changed after qualification')
                self.assertFalse(GATE.reconcile(paths)['closure_allowed'])

    def test_real_reconciliation_rejects_insufficient_measured_coverage(self):
        for kind, name in [('component', 'local-mcp-coverage.json'),
                           ('quality', 'shared-sessions/coverage.json')]:
            with self.subTest(kind=kind), self.evidence_fixture() as (_, paths, reports):
                coverage = paths[kind].parent / name
                value = json.loads(coverage.read_text())
                value['data'][0]['files'][0]['summary']['lines']['covered'] = 94
                coverage.write_text(json.dumps(value))
                reports[kind]['artifact_sha256'][name] = GATE.sha256(coverage)
                paths[kind].write_text(json.dumps(reports[kind]))
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
        changes = [('status', 'QUALIFIED_INSTALLED_INSPECTION'), ('status', 'QUALIFIED_INSTALLED_CANONICAL'),
                   ('session_checks', ['completed']), ('cli_parity', False),
                   ('session_cli_inspection_parity', False), ('installed_client', {}),
                   ('initialize', {}), ('provider_authentication_used', True),
                   ('cg_environment', 'inherited'), ('cleanup', []),
                   ('cleanup', [{'forced': True, 'eof_exit_code': 0}]),
                   ('cleanup', [{'forced': False, 'eof_exit_code': 1}]),
                   ('binary_sha256', {'cg': 'digest'}), ('source_sha256', {})]
        for key, value in changes:
            with self.subTest(key=key, value=value), self.evidence_fixture() as (_, paths, reports):
                reports['installed'][key] = value
                paths['installed'].write_text(json.dumps(reports['installed']))
                with self.assertRaises((ValueError, KeyError)):
                    GATE.inspect('installed', paths['installed'], 'candidate')
                self.assertFalse(GATE.reconcile(paths)['closure_allowed'])

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
