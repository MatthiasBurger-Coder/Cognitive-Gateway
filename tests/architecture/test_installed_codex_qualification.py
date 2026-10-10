"""Runner guardrails; these unit tests never establish installed-client acceptance."""
import copy
import gzip
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/qualify-installed-codex.py'
SPEC = importlib.util.spec_from_file_location('installed_codex', SCRIPT)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class InstalledQualificationGuardrails(unittest.TestCase):
    def test_missing_installed_client_is_not_run_and_nonzero(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'evidence'
            result = subprocess.run([sys.executable, str(SCRIPT), '--codex', str(Path(directory) / 'absent'),
                                     '--output', str(output)], capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            report = json.loads((output / 'report.json').read_text())
            self.assertEqual(report['status'], 'NOT_RUN')
            self.assertFalse(report['inference_started'])

    def test_shared_mode_without_database_is_blocked(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'evidence'
            # Python is a present executable, but must never be launched as Codex here.
            result = subprocess.run([sys.executable, str(SCRIPT), '--codex', sys.executable,
                                     '--shared-session-fixture', '--output', str(output)], env={},
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            report = json.loads((output / 'report.json').read_text())
            self.assertEqual(report['status'], 'BLOCKED')
            self.assertNotIn('installed_client', report)

    def test_equal_count_catalog_replacement_and_wrong_operation_fail(self):
        tools = {}
        for version in ['v1', 'v2']:
            for tool in json.loads((ROOT / f'schemas/codex/{version}/catalog.json').read_text())['tools']:
                tools[tool['name']] = {'annotations': tool['annotations'], 'inputSchema': {
                    'properties': {'operation': {'const': tool['operation']}}}}
        runner.verify_catalog(tools, True)
        replaced = copy.deepcopy(tools)
        replaced['cg_session_authority_v2'] = replaced.pop('cg_session_cancel_v2')
        with self.assertRaises(ValueError):
            runner.verify_catalog(replaced, True)
        wrong = copy.deepcopy(tools)
        wrong['cg_session_cancel_v2']['inputSchema']['properties']['operation']['const'] = 'session.approve'
        with self.assertRaises(ValueError):
            runner.verify_catalog(wrong, True)

    def test_transcript_compression_preserves_exact_requests_and_responses(self):
        transcript = [{'method': 'mcpServer/tool/call', 'request': {'id': 1},
                       'response': {'result': {'structuredContent': {'status': 'error'}}}}]
        with tempfile.TemporaryDirectory() as directory:
            runner.retain_transcript(Path(directory), transcript)
            self.assertEqual(json.loads(gzip.decompress((Path(directory) / 'rpc-evidence.json.gz').read_bytes())),
                             transcript)


if __name__ == '__main__':
    unittest.main()
