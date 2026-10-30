"""Shell adapters must preserve argument boundaries and separate installation/operations."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
INSTALL = 'install/linux-cli.sh'
POSTGRES = 'operations/postgres/'
MODEL = 'operations/local-model/'


@unittest.skipUnless(os.name == "posix" and shutil.which("bash"), "Requires POSIX Bash")
class ShellEntrypoints(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='cg shell ')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.log = self.root / 'calls.jsonl'
        stub = f'#!{sys.executable}\n' + '''import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
entry = {'tool':name, 'args':sys.argv[1:], 'cwd':os.getcwd()}
if name == 'cargo' and sys.argv[1:2] == ['llvm-cov']:
    entry['profiles'] = {key:os.environ.get('CARGO_PROFILE_' + key + '_OPT_LEVEL')
                         for key in ('DEV', 'TEST')}
    entry['compiler_flags'] = {key:os.environ.get(key)
                               for key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')}
with open(os.environ['CG_SHELL_TEST_LOG'], 'a') as output:
    output.write(json.dumps(entry) + '\\n')
if name == 'openssl':
    print('a' * 64)
if name == 'cargo' and sys.argv[1] == 'install':
    root = pathlib.Path(sys.argv[sys.argv.index('--root') + 1]) / 'bin'
    root.mkdir(parents=True)
    for binary in ('cg', 'cg-registry'):
        path = root / binary
        path.write_text('#!/bin/sh\\nexit 0\\n')
        path.chmod(0o755)
if name == 'cargo' and sys.argv[1:3] == ['llvm-cov', 'clean'] and 'CG_SHELL_TEST_CLEAN_EXIT' in os.environ:
    sys.exit(int(os.environ['CG_SHELL_TEST_CLEAN_EXIT']))
sys.exit(int(os.environ.get('CG_SHELL_TEST_EXIT', '0')))
'''
        for tool in ('docker', 'curl', 'cargo', 'openssl'):
            path = self.bin / tool
            path.write_text(stub)
            path.chmod(0o755)
        self.env = {**os.environ, 'PATH':str(self.bin) + os.pathsep + os.environ['PATH'],
                    'CG_SHELL_TEST_LOG':str(self.log), 'CG_MODEL_ACCELERATION':'cpu',
                    'CG_POSTGRES_ENV_FILE':str(self.root / 'config' / 'postgres.env')}
        self.env.pop('CG_SHELL_TEST_EXIT', None)

    def run_script(self, script, *args, **environment):
        return subprocess.run(['bash', str(ROOT / script), *args], cwd=self.root,
                              env={**self.env, **environment}, capture_output=True, text=True,
                              timeout=15)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_all_shell_entrypoints_offer_side_effect_free_help(self):
        scripts = sorted((*ROOT.glob('install/*.sh'), *ROOT.glob('operations/**/*.sh'), *ROOT.glob('tests/scripts/*.sh')))
        self.assertTrue(scripts)
        for script in scripts:
            with self.subTest(script=script):
                output = self.run_script(str(script), '--help')
                self.assertEqual(output.returncode, 0, output.stderr)
                self.assertIn('Usage:', output.stdout)
        self.assertEqual(self.calls(), [])
        self.assertFalse(Path(self.env['CG_POSTGRES_ENV_FILE']).exists())

    def test_native_qualifications_use_one_profile_before_building(self):
        for runner in ('shared-sessions', 'observed-goal-sessions', 'before-journal', 'atomic-before'):
            with self.subTest(runner=runner):
                result = self.run_script('tests/scripts/qualify-' + runner + '.sh',
                    str(self.root / runner), CG_COGNITIVE_TEST_DATABASE='test-host-supplied',
                    CARGO_LLVM_COV_TARGET_DIR=str(self.root / 'coverage'),
                    CARGO_PROFILE_DEV_OPT_LEVEL='0', CARGO_PROFILE_TEST_OPT_LEVEL='2',
                    CG_SHELL_TEST_EXIT='17')
                self.assertEqual(result.returncode, 17, result.stderr)
                call = self.calls()[-1]
                self.assertEqual(call['tool'], 'cargo')
                self.assertEqual(call['args'], ['llvm-cov', 'clean', '--workspace'])
                self.assertEqual(call['profiles'], {'DEV':'1', 'TEST':'1'})

    def test_native_qualifications_keep_function_bodies_and_inherited_compiler_flags(self):
        self.env.pop('RUSTFLAGS', None)
        self.env.pop('CARGO_ENCODED_RUSTFLAGS', None)
        cases = [
            ({'RUSTFLAGS':'-C debuginfo=1'},
             {'RUSTFLAGS':'-C debuginfo=1 -C link-dead-code', 'CARGO_ENCODED_RUSTFLAGS':None}),
            ({'RUSTFLAGS':'-C debuginfo=2', 'CARGO_ENCODED_RUSTFLAGS':'-C\x1fdebuginfo=1'},
             {'RUSTFLAGS':'-C debuginfo=2',
              'CARGO_ENCODED_RUSTFLAGS':'-C\x1fdebuginfo=1\x1f-C\x1flink-dead-code'}),
            ({'CARGO_ENCODED_RUSTFLAGS':''},
             {'RUSTFLAGS':None, 'CARGO_ENCODED_RUSTFLAGS':'-C\x1flink-dead-code'}),
        ]
        for runner in ('shared-sessions', 'before-journal', 'atomic-before'):
            for inherited, expected in cases:
                with self.subTest(runner=runner, flags=inherited):
                    result = self.run_script('tests/scripts/qualify-' + runner + '.sh',
                        str(self.root / runner), CG_COGNITIVE_TEST_DATABASE='test-host-supplied',
                        CARGO_LLVM_COV_TARGET_DIR=str(self.root / 'coverage'),
                        CG_SHELL_TEST_EXIT='17', **inherited)
                    self.assertEqual(result.returncode, 17, result.stderr)
                    self.assertEqual(self.calls()[-1]['compiler_flags'], expected)

    def test_single_cargo_qualifications_use_the_shared_compiler_environment(self):
        self.env.pop('CARGO_ENCODED_RUSTFLAGS', None)
        for runner in ('observed-goal-sessions', 'complete-snapshots'):
            with self.subTest(runner=runner):
                result = self.run_script('tests/scripts/qualify-' + runner + '.sh',
                    str(self.root / runner), CG_COGNITIVE_TEST_DATABASE='test-host-supplied',
                    CARGO_LLVM_COV_TARGET_DIR=str(self.root / 'coverage'),
                    RUSTFLAGS='-C debuginfo=1', CG_SHELL_TEST_CLEAN_EXIT='0', CG_SHELL_TEST_EXIT='17')
                self.assertEqual(result.returncode, 17, result.stderr)
                self.assertEqual(self.calls()[-1]['compiler_flags'],
                                 {'RUSTFLAGS':'-C debuginfo=1 -C link-dead-code',
                                  'CARGO_ENCODED_RUSTFLAGS':None})

    def test_declared_rust_coverage_builds_use_the_shared_environment(self):
        for filename in ('quality-gates.json', 'invocation-acceptance.json'):
            data = json.loads((ROOT / 'tests/scripts' / filename).read_text())
            gates = data if isinstance(data, list) else data['gates']
            for gate in gates:
                if filename == 'quality-gates.json':
                    self.assertNotIn('CARGO_LLVM_COV_TARGET_DIR=', gate['command'])
                for line in gate['command'].splitlines():
                    prefix, found, arguments = line.partition('cargo llvm-cov ')
                    if not found or arguments.split()[0] in ('clean', 'report', 'show-env'):
                        continue
                    with self.subTest(manifest=filename, gate=gate.get('name', gate.get('id'))):
                        self.assertTrue(prefix.rstrip().endswith('tests/scripts/with-coverage-env.sh'))

    def test_domain_coverage_uses_the_shared_compiler_environment(self):
        self.env.pop('CARGO_ENCODED_RUSTFLAGS', None)
        gates = json.loads((ROOT / 'tests/scripts/quality-gates.json').read_text())
        command = next(gate['command'] for gate in gates if gate['name'] == 'Domain coverage gate')
        env = {**self.env, 'RUSTFLAGS':'-C debuginfo=1',
               'EVIDENCE_DIR':str(self.root / 'domain evidence'),
               'CG_SHELL_TEST_CLEAN_EXIT':'0', 'CG_SHELL_TEST_EXIT':'17'}
        result = subprocess.run(['bash', '-euo', 'pipefail', '-c', command],
                                cwd=ROOT, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 17, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0]['args'], ['llvm-cov', 'clean', '--workspace'])
        self.assertEqual(calls[1]['compiler_flags'],
                         {'RUSTFLAGS':'-C debuginfo=1 -C link-dead-code',
                          'CARGO_ENCODED_RUSTFLAGS':None})
        self.assertIn(str(self.root / 'domain evidence/domain-coverage.json'), calls[1]['args'])

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux-only installer')
    def test_installer_only_installs_requested_clis_and_preserves_path_boundaries(self):
        destination = self.root / 'installed tools'
        result = self.run_script(INSTALL, '--root', str(destination))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [{'tool':'cargo', 'args':['install', '--path',
            str(ROOT / 'crates/gateway-daemon'), '--bin', 'cg', '--bin', 'cg-registry',
            '--locked', '--force', '--root', str(destination)], 'cwd':str(self.root)}])
        self.assertTrue((destination / 'bin/cg').is_file())
        self.assertEqual(self.run_script(INSTALL, '--with-postgres').returncode, 2)

    def test_invalid_arguments_never_invoke_external_tools(self):
        for script, args in [(INSTALL, ['--root']), (INSTALL, ['--root', 'relative']),
                ('tests/scripts/with-coverage-env.sh', []),
                ('tests/scripts/with-coverage-env.sh', ['--unknown']),
                (POSTGRES+'start.sh', ['extra']), (MODEL+'service.sh', ['unknown']),
                (MODEL+'service.sh', ['stop', 'extra']), (MODEL+'models.sh', ['start']),
                (MODEL+'models.sh', ['promote']), (MODEL+'models.sh', ['inspect', 'extra'])]:
            with self.subTest(script=script, args=args):
                self.assertEqual(self.run_script(script, *args).returncode, 2)
        for script in ROOT.glob('tests/scripts/*.sh'):
            with self.subTest(test_runner=script):
                self.assertEqual(self.run_script(str(script), '--unknown').returncode, 2)
        self.assertEqual(self.calls(), [])

    def test_model_compose_selects_overlays_and_propagates_failure(self):
        for acceleration, overlay in [('cpu',None), ('nvidia','gpu'), ('gpu','gpu'),
                                      ('amd','amd'), ('vulkan','vulkan')]:
            result = self.run_script(MODEL+'compose.sh', 'config', '--quiet',
                                     CG_MODEL_ACCELERATION=acceleration)
            self.assertEqual(result.returncode, 0, result.stderr)
            expected = ['compose', '--project-directory', str(ROOT), '-f', str(ROOT/'compose.model.yaml')]
            if overlay:
                expected += ['-f', str(ROOT/f'compose.model.{overlay}.yaml')]
            self.assertEqual(self.calls()[-1]['args'], expected+['config','--quiet'])
        count = len(self.calls())
        self.assertEqual(self.run_script(MODEL+'compose.sh', 'config', CG_MODEL_ACCELERATION='invalid').returncode, 2)
        self.assertEqual(len(self.calls()), count)
        self.assertEqual(self.run_script(MODEL+'compose.sh', 'ps', CG_SHELL_TEST_EXIT='7').returncode, 7)

    def test_service_and_model_operations_are_separate(self):
        for operation, expected in [('start',['up','-d','--build','--wait','--wait-timeout','360']),
                                     ('stop',['stop']), ('logs',['logs','--tail','100']),
                                     ('config',['config'])]:
            result = self.run_script(MODEL+'service.sh', operation)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.calls()[-1]['args'][-len(expected):], expected)
        result = self.run_script(MODEL+'service.sh', 'ready', CG_MODEL_SERVICE_PORT='18091')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls()[-1]['tool'], 'curl')
        self.assertEqual(self.calls()[-1]['args'][-1], 'http://127.0.0.1:18091/ready')
        for operation, target in [('install','/models/profile with spaces.json'), ('test','model-id'),
                                  ('promote','model-id'), ('rollback','role'), ('disable','model-id'),
                                  ('inspect',None)]:
            args = [operation] + ([target] if target else [])
            result = self.run_script(MODEL+'models.sh', *args)
            self.assertEqual(result.returncode, 0, result.stderr)
            expected = ['exec','-T','model-service','python','service.py']+args
            self.assertEqual(self.calls()[-1]['args'][-len(expected):], expected)

    @unittest.skipUnless(sys.platform.startswith("linux"), "PostgreSQL startup requires GNU stat")
    def test_postgres_initialization_reuses_credentials_and_compose_is_explicit(self):
        env_file = Path(self.env['CG_POSTGRES_ENV_FILE'])
        self.assertEqual(self.run_script(POSTGRES+'compose.sh', 'ps').returncode, 1)
        self.assertEqual(self.calls(), [])
        result = self.run_script(POSTGRES+'start.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        before = env_file.read_bytes()
        self.assertNotIn('CG_POSTGRES_PASSWORD=', result.stdout)
        self.assertEqual(env_file.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.run_script(POSTGRES+'start.sh').returncode, 0)
        self.assertEqual(env_file.read_bytes(), before)
        self.assertEqual(sum(c['tool']=='openssl' for c in self.calls()), 1)
        self.assertEqual(self.run_script(POSTGRES+'compose.sh', 'ps').returncode, 0)
        self.assertEqual(self.calls()[-1]['args'], ['compose','--project-directory',str(ROOT),
            '--env-file',str(env_file),'-f',str(ROOT/'compose.yaml'),'ps'])
        env_file.write_text('CG_POSTGRES_PASSWORD=replace-with-a-long-random-password\n')
        count = len(self.calls())
        self.assertEqual(self.run_script(POSTGRES+'start.sh').returncode, 1)
        self.assertEqual(len(self.calls()), count+1)  # Docker availability check only.
