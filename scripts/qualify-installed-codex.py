#!/usr/bin/env python3
"""Qualify a real installed Codex MCP client without inference or provider auth.

This establishes shipped inspection/canonical interoperability, not shared session completion.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def proxy(args):
    """Observe initialize metadata only; forward protocol bytes unchanged to CG."""
    metadata, binary, *launch = args
    first = sys.stdin.buffer.readline(1_048_577)
    if len(first) > 1_048_576:
        return 2
    message = json.loads(first)
    params = message.get('params', {})
    identity = params.get('clientInfo', {})
    # The trusted launch binds the expected identity; observed identity is not a grant.
    expected_name = launch[launch.index('--client-name') + 1]
    expected_version = launch[launch.index('--client-version') + 1]
    if (message.get('method') != 'initialize' or identity.get('name') != expected_name
            or identity.get('version') != expected_version):
        return 2
    observed = {'protocol_version': params.get('protocolVersion'), 'client_name': identity['name'],
                'client_version': identity['version'], 'discovery_requests': []}
    Path(metadata).write_text(json.dumps(observed) + '\n')
    child = subprocess.Popen([binary, *launch], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=None, env={})
    child.stdin.write(first)
    child.stdin.flush()

    def forward_input():
        try:
            while line := sys.stdin.buffer.readline(1_048_577):
                if len(line) > 1_048_576:
                    break
                request = json.loads(line)
                if request.get('method') in ('tools/list', 'resources/list', 'resources/templates/list'):
                    fields = request.get('params') or {}
                    observed['discovery_requests'].append({'method': request['method'],
                        'known_fields': {key: 'null' if fields[key] is None else type(fields[key]).__name__
                                         for key in ('cursor', '_meta') if key in fields},
                        'other_field_count': len(set(fields) - {'cursor', '_meta'})})
                    Path(metadata).write_text(json.dumps(observed) + '\n')
                child.stdin.write(line)
                child.stdin.flush()
        except (BrokenPipeError, OSError):
            pass
        finally:
            child.stdin.close()

    threading.Thread(target=forward_input, daemon=True).start()
    for line in child.stdout:
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    return child.wait(timeout=5)


class AppServer:
    def __init__(self, binary, home):
        self.process = subprocess.Popen(
            [str(binary), 'app-server', '--stdio'], cwd=home, text=True,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env={'PATH': os.environ.get('PATH', ''), 'HOME': str(home), 'CODEX_HOME': str(home)},
            start_new_session=True)
        self.queue = queue.Queue()
        self.counter = 0
        self.transcript = []

        def read():
            for line in self.process.stdout:
                try:
                    self.queue.put(json.loads(line))
                except ValueError:
                    self.queue.put({'error': {'code': 'INVALID_JSON'}})
            self.queue.put(None)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()

    def notify(self, method):
        self.process.stdin.write(json.dumps({'method': method}) + '\n')
        self.process.stdin.flush()

    def call(self, method, params):
        self.counter += 1
        command = {'id': self.counter, 'method': method, 'params': params}
        self.process.stdin.write(json.dumps(command) + '\n')
        self.process.stdin.flush()
        deadline = time.monotonic() + 20
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError()
            reply = self.queue.get(timeout=remaining)
            if reply is None:
                raise ValueError('app-server disconnected')
            if reply.get('id') == self.counter:
                self.transcript.append({'method': method, 'response': reply})
                if 'error' in reply:
                    raise ValueError('app-server operation failed')
                return reply['result']

    def close(self):
        if not self.process.stdin.closed:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(self.process.pid, signal.SIGTERM)
            self.process.wait(timeout=5)
        finally:
            # End any child left in our isolated process group, including the proxy.
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            self.reader.join(timeout=2)
            self.process.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex', type=Path)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--repository', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    parser.add_argument('--client-name', default='codex-mcp-client')
    parser.add_argument('--canonical-fixture', action='store_true',
                        help='Also qualify resolve/explain/compile with explicit neutral fixture inputs')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {'scope': 'installed Codex -> shipped CG inspection', 'status': 'RUNNING',
              'epic_04_status': 'NOT_COMPLETE', 'inference_started': False,
              'cg_environment': 'empty', 'provider_authentication_used': False,
              'limitations': ['Canonical resolve/explain/context and shared-session lifecycle require separate qualification.',
                              'This local inspection proof does not qualify EPIC-08 or model/connector behavior.'],
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'started_at_unix': time.time()}
    app = None
    fixture = None
    try:
        found = str(args.codex) if args.codex else shutil.which('codex')
        if not found:
            report['status'] = 'NOT_RUN'
            raise FileNotFoundError()
        codex = Path(found).resolve(strict=True)
        version = subprocess.check_output([str(codex), '--version'], text=True, timeout=10).strip()
        if not version.startswith('codex-cli '):
            raise ValueError('unknown installed client version')
        client_version = version.removeprefix('codex-cli ')
        report['installed_client'] = {'launcher_path': str(codex), 'launcher_sha256': sha256(codex), 'version': version}
        report['source_sha256'] = {str(path.relative_to(ROOT)): sha256(path) for path in
                                   sorted((ROOT / 'crates').rglob('*.rs'))}
        report['source_sha256'].update({str(path.relative_to(ROOT)): sha256(path)
            for base in ['schemas/codex', 'tests/fixtures/declarative-cli', 'tests/codex-local', 'scripts']
            for path in sorted((ROOT / base).rglob('*'))
            if path.is_file() and path.suffix in ('.py', '.json', '.jsonl', '.toml')})
        with tempfile.TemporaryDirectory(prefix='cg-installed-codex-') as temporary:
            home = Path(temporary)
            bootstrap = home / 'bootstrap'
            subprocess.run([sys.executable, str(ROOT / 'scripts/bootstrap-codex-local.py'),
                            '--repository', str(args.repository.resolve(strict=True)),
                            '--output', str(bootstrap), '--client-name', args.client_name,
                            '--client-version', client_version, '--bin-dir', str(args.bin_dir.resolve())],
                           capture_output=True, timeout=15, check=True)
            launch = json.loads((bootstrap / 'launch.json').read_text())
            request = json.loads((bootstrap / 'request.json').read_text())
            if args.canonical_fixture:
                # Reuse the same admitted fixture as the shipped-host regression.
                # Fixture data replaces operator snapshots, never the product host.
                os.environ['CG_QUALIFICATION_BIN_DIR'] = str(args.bin_dir.resolve())
                sys.path.insert(0, str(ROOT / 'tests/codex-local'))
                from test_canonical_host import CanonicalHost
                fixture = CanonicalHost()
                fixture.setUp()
                fixture.prepare()
                launch, request = fixture.launch, fixture.request
                launch[launch.index('--client-name') + 1] = args.client_name
                launch[launch.index('--client-version') + 1] = client_version
                (bootstrap / 'request.json').write_text(json.dumps(request))
                report['scope'] = 'installed Codex -> shipped CG inspection and canonical operations'
                report['substitutions'] = ['Neutral admitted plan/rules/process/policy/projection snapshots and catalog; no application host substitution.']
                report['limitations'][0] = 'Shared-session lifecycle requires separate qualification; this run does not supply session services.'
            metadata = output / 'mcp-initialize.json'
            command = shutil.which('env')
            proxy_args = ['-i', sys.executable, str(Path(__file__).resolve()), '--proxy-initialize',
                          str(metadata), str((args.bin_dir / 'cg-mcp').resolve()), *launch]
            config = '[mcp_servers.cognitive_gateway]\ncommand = ' + json.dumps(command) + '\n'
            config += 'args = ' + json.dumps(proxy_args) + '\nstartup_timeout_sec = 10\ntool_timeout_sec = 10\n'
            config += '[analytics]\nenabled = false\n'
            (home / 'config.toml').write_text(config)
            app = AppServer(codex, home)
            app.call('initialize', {'clientInfo': {'name': 'cg_qualification', 'version': '1.0'},
                                    'capabilities': {'experimentalApi': True}})
            app.notify('initialized')
            thread = app.call('thread/start', {'cwd': str(home), 'ephemeral': True,
                                              'sandbox': 'read-only', 'approvalPolicy': 'never',
                                              'baseInstructions': 'Local interoperability qualification; no inference.'})['thread']['id']
            deadline = time.monotonic() + 15
            while True:
                status = app.call('mcpServerStatus/list', {'threadId': thread})
                server = next(entry for entry in status['data'] if entry['name'] == 'cognitive_gateway')
                if server['runtimeStatus'] != 'starting' or time.monotonic() >= deadline:
                    break
                time.sleep(0.1)
            if server['runtimeStatus'] != 'connected' or len(server['tools']) != 13:
                raise ValueError('installed client did not discover CG')
            result = app.call('mcpServer/tool/call', {'threadId': thread, 'server': 'cognitive_gateway',
                                                    'tool': 'cg_situation_inspect_v1', 'arguments': request})
            cli = subprocess.run([str((args.bin_dir / 'cg-local').resolve()), '--operation', 'situation.inspect',
                                  '--request', str(bootstrap / 'request.json'), *launch],
                                 env={}, capture_output=True, text=True, timeout=10, check=True)
            canonical = json.loads(cli.stdout)
            if result.get('isError') or result.get('structuredContent') != canonical or canonical['status'] != 'ok':
                raise ValueError('installed client and CLI results differ')
            foreign = json.loads(json.dumps(request))
            foreign['scope']['workspace_id'] = 'foreign-workspace'
            denied = app.call('mcpServer/tool/call', {'threadId': thread, 'server': 'cognitive_gateway',
                                                    'tool': 'cg_situation_inspect_v1', 'arguments': foreign})
            if (not denied.get('isError')
                    or denied['structuredContent']['diagnostics'][0]['code'] != 'CG_SCOPE_DENIED'):
                raise ValueError('scope refusal missing')
            if fixture:
                checks = []
                def invoke(operation, arguments):
                    response = app.call('mcpServer/tool/call', {'threadId': thread, 'server': 'cognitive_gateway',
                        'tool': 'cg_' + operation.replace('.', '_') + '_v1', 'arguments': arguments})
                    return response['structuredContent']
                resolve_request = fixture.request_for('capabilities.resolve',
                    {name: fixture.references[name] for name in ['plan', 'rules', 'process']})
                resolved = invoke('capabilities.resolve', resolve_request)
                if resolved['status'] != 'ok' or resolved != fixture.cli_envelope('capabilities.resolve', resolve_request):
                    raise ValueError('installed resolve parity missing')
                reference = next(p['reference'] for p in resolved['provenance'] if p['reference']['id'] == 'local-resolution')
                commands = [('state.explain', {'resolution': reference}),
                            ('context.compile', {'resolution': reference, 'projection': fixture.references['projection'],
                                                 'step_id': 'step-condition.0', 'candidates': []})]
                checks.append('capabilities.resolve: complete CLI/MCP envelope parity')
                for operation, inputs in commands:
                    envelope = fixture.request_for(operation, inputs)
                    response = invoke(operation, envelope)
                    if response['status'] != 'ok' or response != fixture.cli_envelope(operation, envelope):
                        raise ValueError('installed canonical parity missing')
                    checks.append(operation + ': complete CLI/MCP envelope parity')
                stale = copy.deepcopy(fixture.request_for('state.explain', {'resolution': reference}))
                stale['input']['resolution']['digest'] = 'sha256:' + '0' * 64
                if invoke('state.explain', stale)['diagnostics'][0]['code'] != 'CG_STALE_REVISION':
                    raise ValueError('stale resolution accepted')
                report['canonical_checks'] = checks + ['stale resolution: CG_STALE_REVISION']
            report['initialize'] = json.loads(metadata.read_text())
            report['discovered_tools'] = len(server['tools'])
            report['canonical_response'] = canonical
            report['scope_denial'] = 'CG_SCOPE_DENIED'
            report['cli_parity'] = True
            report['binary_sha256'] = {name: sha256((args.bin_dir / name).resolve()) for name in ['cg', 'cg-mcp', 'cg-local']}
            report['status'] = 'QUALIFIED_INSTALLED_CANONICAL' if fixture else 'QUALIFIED_INSTALLED_INSPECTION'
            # Retain only admitted fixture results and protocol metadata, no raw auth/config.
            (output / 'rpc-evidence.json').write_text(json.dumps(app.transcript, indent=2) + '\n')
    except (OSError, ValueError, KeyError, StopIteration, queue.Empty, subprocess.SubprocessError):
        if report['status'] != 'NOT_RUN':
            report['status'] = 'FAIL'
        report['diagnostic'] = 'Installed-client qualification did not complete; no success inferred.'
    finally:
        if app:
            (output / 'rpc-evidence.json').write_text(json.dumps(app.transcript, indent=2) + '\n')
            app.close()
        if fixture:
            fixture.doCleanups()
        report['finished_at_unix'] = time.time()
        if report['status'] in ('QUALIFIED_INSTALLED_INSPECTION', 'QUALIFIED_INSTALLED_CANONICAL'):
            unchanged = all((ROOT / name).is_file() and sha256(ROOT / name) == digest
                            for name, digest in report['source_sha256'].items())
            unchanged = unchanged and all(sha256((args.bin_dir / name).resolve()) == digest
                                          for name, digest in report['binary_sha256'].items())
            unchanged = unchanged and sha256(codex) == report['installed_client']['launcher_sha256']
            if not unchanged:
                report['status'] = 'FAIL'
                report['diagnostic'] = 'Candidate sources or executables changed during qualification.'
        report['artifact_sha256'] = {path.name: sha256(path) for path in output.iterdir() if path.is_file()}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{report['status']}: {output / 'report.json'}")
    return 0 if report['status'] in ('QUALIFIED_INSTALLED_INSPECTION', 'QUALIFIED_INSTALLED_CANONICAL') else 1


if __name__ == '__main__':
    if sys.argv[1:2] == ['--proxy-initialize']:
        sys.exit(proxy(sys.argv[2:]))
    sys.exit(main())
