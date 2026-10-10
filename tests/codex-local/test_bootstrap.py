"""Operator artifacts must be reproducible, secret-free, and non-destructive."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BootstrapTests(unittest.TestCase):
    def run_bootstrap(self, repository, output, version='1.0', bin_dir=None):
        command = ['python3', str(ROOT / 'scripts/bootstrap-codex-local.py'),
                   '--repository', str(repository), '--output', str(output),
                   '--client-name', 'codex', '--client-version', version]
        if bin_dir:
            command += ['--bin-dir', str(bin_dir)]
        return subprocess.run(command, cwd=ROOT, capture_output=True, text=True,
                              env={**os.environ, 'OPENAI_API_KEY': 'DO_NOT_ECHO'}, timeout=20)

    def test_config_clears_environment_and_admits_only_sample(self):
        with tempfile.TemporaryDirectory(prefix='cg local spaces ') as tmp:
            output = Path(tmp) / 'setup'
            result = self.run_bootstrap(tmp, output)
            self.assertEqual(result.returncode, 0, result.stderr)
            config = tomllib.loads((output / 'config.toml').read_text())
            server = config['mcp_servers']['cognitive_gateway']
            self.assertEqual(server['args'][0], '-i')
            self.assertEqual(server['env_vars'], [])
            self.assertTrue(Path(server['args'][1]).is_absolute())
            admission = json.loads((output / 'admission.json').read_text())
            self.assertEqual(admission['mappings'][0]['repository'], str(Path(tmp).resolve()))
            self.assertEqual(len(admission['mappings'][0]['resources']), 1)
            self.assertEqual(admission['mappings'][0]['resources'][0]['provenance'][0]['sensitivity'], 'PUBLIC')
            for path in output.iterdir():
                self.assertNotIn('DO_NOT_ECHO', path.read_text())
            smoke = subprocess.run(['python3', str(ROOT / 'scripts/check-codex-local.py'),
                                    '--setup', str(output)], cwd=ROOT, capture_output=True,
                                   text=True, timeout=45)
            self.assertEqual(smoke.returncode, 0, smoke.stderr)
            before = {p.name: p.read_bytes() for p in output.iterdir()}
            again = self.run_bootstrap(tmp, output)
            self.assertEqual(again.returncode, 2)
            self.assertEqual(before, {p.name: p.read_bytes() for p in output.iterdir()})

    def test_missing_binary_root_and_bad_identity_have_safe_diagnostics(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            for repository, output, version, bin_dir in [
                    (base / 'missing', base / 'one', '1.0', None),
                    (base, base / 'two', '1.0', base / 'missing'),
                    (base, base / 'three', 'Bearer DO_NOT_ECHO', None)]:
                result = self.run_bootstrap(repository, output, version, bin_dir)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, '')
                self.assertNotIn('DO_NOT_ECHO', result.stderr)
                self.assertFalse((output / 'config.toml').exists())
                self.assertFalse((output / 'launch.json').exists())
        for script in ['bootstrap-codex-local.py', 'check-codex-local.py']:
            result = subprocess.run(['python3', str(ROOT / 'scripts' / script),
                                     '--api-key', 'DO_NOT_ECHO'], capture_output=True,
                                    text=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn('DO_NOT_ECHO', result.stderr)


if __name__ == '__main__':
    unittest.main()
