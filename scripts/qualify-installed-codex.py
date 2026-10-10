#!/usr/bin/env python3
"""Qualify a real installed Codex MCP client without inference or provider auth.

Optional shared-session qualification uses the real disposable PostgreSQL host.
"""
import argparse
import copy
import gzip
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


def retain_transcript(output, transcript):
    # Discovery includes complete bundled schemas; retain exact RPC data compactly.
    (output / 'rpc-evidence.json.gz').write_bytes(
        gzip.compress((json.dumps(transcript, indent=2) + '\n').encode(), mtime=0))


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
        response = json.loads(line)
        if response.get('id') == message.get('id') and 'result' in response:
            observed['negotiated_protocol_version'] = response['result'].get('protocolVersion')
            Path(metadata).write_text(json.dumps(observed) + '\n')
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
                self.transcript.append({'method': method, 'request': command, 'response': reply})
                if 'error' in reply:
                    if reply['error'].get('code') == -32601:
                        raise NotImplementedError('required app-server API unavailable')
                    raise ValueError('app-server operation failed')
                return reply['result']

    def close(self):
        started = time.monotonic()
        forced = False
        if not self.process.stdin.closed:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            forced = True
            os.killpg(self.process.pid, signal.SIGTERM)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=2)
        finally:
            # End any child left in our isolated process group, including the proxy.
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            self.reader.join(timeout=2)
            self.process.stdout.close()
        return {'eof_exit_code': self.process.returncode, 'forced': forced,
                'elapsed_seconds': time.monotonic() - started}


def native_identity(process):
    """Hash the running executable, including npm launcher's native child on Linux."""
    pending = [process.pid]
    while pending:
        pid = pending.pop()
        path = Path(f'/proc/{pid}/exe').resolve(strict=True)
        if path.name == 'codex':
            return {'path': str(path), 'sha256': sha256(path)}
        pending.extend(int(p) for p in Path(f'/proc/{pid}/task/{pid}/children').read_text().split())
    raise ValueError('cannot identify running native Codex binary')


def verify_catalog(tools, sessions=False):
    expected = {}
    for version in (['v1', 'v2'] if sessions else ['v1']):
        for tool in json.loads((ROOT / f'schemas/codex/{version}/catalog.json').read_text())['tools']:
            expected[tool['name']] = tool
    if set(tools) != set(expected):
        raise ValueError('installed discovery differs from frozen catalog')
    for name, tool in tools.items():
        # Codex's app-server projection omits MCP execution metadata.
        if tool['annotations'] != expected[name]['annotations']:
            raise ValueError('installed tool semantics differ from catalog')
        schema = tool['inputSchema']
        branches = schema.get('oneOf', [schema])
        if len(branches) != 1 or branches[0]['properties']['operation'] != {'const': expected[name]['operation']}:
            raise ValueError('installed tool operation differs from catalog')


def qualify_sessions(fixture, invoke, reconnect):
    """Every lifecycle call uses the installed client; CLI only inspects or issues authority."""
    checks = []
    def state(result, expected):
        if result['status'] != 'ok' or result['result']['session']['state'] != expected:
            raise ValueError('unexpected installed session transition')
        inspected = invoke(fixture.inspect(result))
        if inspected != fixture.cli(fixture.inspect(result)) or inspected['result'] != result['result']:
            raise ValueError('installed session inspection parity missing')
        checks.append(expected)
        return result

    def denied(request, code):
        result = invoke(request)
        if result['status'] != 'error' or result['diagnostics'][0]['code'] != code:
            raise ValueError('installed session refusal missing: ' + code)
        checks.append(code)

    start = fixture.request_v2('start', {'command_id': 'installed-start', 'intent': {
        'kind': 'document', 'contract': 'cg.intent', 'contract_version': '1.0', 'document': fixture.intent}})
    started = state(invoke(start), 'pending_clarification')
    denied(start, 'CG_DUPLICATE_COMMAND')
    answer = fixture.mutate('clarify', started, 'installed-answer',
        pending_id=started['result']['session']['pending']['id'], answer={
            'contract': 'cg.clarification-answer', 'contract_version': '2.0',
            'selected_source': fixture.config['basis']['sources'][1]})
    wrong = copy.deepcopy(answer)
    wrong['input']['pending_id'] = 'wrong'
    denied(wrong, 'CG_INVALID_INTERACTION')
    answered = state(invoke(answer), 'runnable')
    denied(answer, 'CG_DUPLICATE_COMMAND')
    pending = state(invoke(fixture.mutate('continue', answered, 'installed-pause')), 'pending_consent')
    reconnect()
    state(invoke(fixture.inspect(pending)), 'pending_consent')
    issued = fixture.operator(pending, 'approve')
    if issued['status'] != 'ok':
        raise ValueError('trusted operator did not issue consent')
    approve = fixture.mutate('approve', pending, 'installed-approve',
        pending_id=pending['result']['session']['pending']['id'], consent={
            'contract': 'cg.consent-record', 'contract_version': '2.0',
            'reference': issued['result']['reference']})
    wrong = copy.deepcopy(approve)
    wrong['input']['consent']['reference']['digest'] = 'sha256:' + '0' * 64
    if invoke(wrong)['status'] != 'error':
        raise ValueError('forged consent accepted')
    accepted = state(invoke(approve), 'runnable')
    denied(approve, 'CG_DUPLICATE_COMMAND')
    stale = fixture.mutate('continue', pending, 'installed-stale')
    denied(stale, 'CG_STALE_REVISION')
    done = state(invoke(fixture.mutate('continue', accepted, 'installed-run')), 'completed')
    if done['result']['budget']['actions'] != 1 or not done['result']['session']['final_evidence']:
        raise ValueError('verified result or bounded action missing')
    reconnect()
    state(invoke(fixture.inspect(done)), 'completed')
    cancelled_start = copy.deepcopy(start)
    cancelled_start['input']['command_id'] = 'installed-cancel-start'
    second = state(invoke(cancelled_start), 'pending_clarification')
    reconnect()
    state(invoke(fixture.inspect(second)), 'pending_clarification')
    cancelled = state(invoke(fixture.mutate('cancel', second, 'installed-cancel')), 'cancelled')
    denied(fixture.mutate('continue', cancelled, 'installed-terminal'), 'CG_INVALID_SESSION_STATE')
    foreign = fixture.inspect(done)
    foreign['scope']['workspace_id'] = 'foreign'
    denied(foreign, 'CG_SCOPE_DENIED')
    unrelated = copy.deepcopy(start)
    unrelated['input']['command_id'] = 'installed-unrelated-goal'
    unrelated['input']['intent']['document']['desired_state']['conditions'][0]['subject'] = 'architecture.clean'
    denied(unrelated, 'CG_UNSUPPORTED_CAPABILITY')
    return {'checks': checks, 'completed': done}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex', type=Path)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--repository', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    parser.add_argument('--client-name', default='codex-mcp-client')
    parser.add_argument('--canonical-fixture', action='store_true',
                        help='Also qualify resolve/explain/compile with explicit neutral fixture inputs')
    parser.add_argument('--shared-session-fixture', action='store_true',
                        help='Qualify canonical and all six session tools; requires disposable PostgreSQL')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {'scope': 'installed Codex -> shipped CG inspection', 'status': 'RUNNING',
              'epic_04_status': 'NOT_COMPLETE', 'closure_allowed': False, 'inference_started': False,
              'cg_environment': 'empty', 'provider_authentication_used': False,
              'limitations': ['Canonical resolve/explain/context and shared-session lifecycle require separate qualification.',
                              'This local inspection proof does not qualify EPIC-08 or model/connector behavior.'],
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'started_at_unix': time.time()}
    report['command'] = [sys.executable, *sys.argv]
    report['cleanup'] = []
    report['source_dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))
    app = None
    fixture = None
    try:
        found = str(args.codex) if args.codex else shutil.which('codex')
        if not found:
            report['status'] = 'NOT_RUN'
            raise FileNotFoundError()
        if args.shared_session_fixture and not os.environ.get('CG_COGNITIVE_TEST_DATABASE'):
            report['status'] = 'BLOCKED'
            raise FileNotFoundError('disposable PostgreSQL host required')
        if not Path(found).is_file() or not os.access(found, os.X_OK):
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
        report['source_sha256'].update({str(path.relative_to(ROOT)): sha256(path)
            for path in [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *(ROOT / 'crates').glob('*/Cargo.toml')]})
        if not all((args.bin_dir / name).is_file() for name in ['cg', 'cg-mcp', 'cg-local']):
            report['status'] = 'BLOCKED'
            raise FileNotFoundError('build the candidate executables')
        report['binary_sha256'] = {name: sha256((args.bin_dir / name).resolve()) for name in ['cg', 'cg-mcp', 'cg-local']}
        if not Path('/proc/self/exe').exists():
            report['status'] = 'BLOCKED'
            raise NotImplementedError('native identity capture currently requires Linux procfs')
        with tempfile.TemporaryDirectory(prefix='cg-installed-codex-') as temporary:
            home = Path(temporary)
            subprocess.run([str(codex), 'app-server', 'generate-json-schema', '--out', str(home / 'schema')],
                env={'PATH': os.environ.get('PATH', ''), 'HOME': str(home), 'CODEX_HOME': str(home)},
                capture_output=True, timeout=15, check=True)
            report['client_api_schema_sha256'] = {str(path.relative_to(home / 'schema')): sha256(path)
                for path in sorted((home / 'schema/v2').glob('*Mcp*.json'))}
            api = home / 'schema/ClientRequest.json'
            if not api.is_file() or not all(method in api.read_text() for method in
                    ['mcpServer/tool/call', 'mcpServer/resource/read', 'mcpServerStatus/list']):
                raise NotImplementedError('required local MCP APIs absent from installed schema')
            report['client_api_schema_sha256']['ClientRequest.json'] = sha256(api)
            bootstrap = home / 'bootstrap'
            subprocess.run([sys.executable, str(ROOT / 'scripts/bootstrap-codex-local.py'),
                            '--repository', str(args.repository.resolve(strict=True)),
                            '--output', str(bootstrap), '--client-name', args.client_name,
                            '--client-version', client_version, '--bin-dir', str(args.bin_dir.resolve())],
                           capture_output=True, timeout=15, check=True)
            launch = json.loads((bootstrap / 'launch.json').read_text())
            request = json.loads((bootstrap / 'request.json').read_text())
            if args.canonical_fixture or args.shared_session_fixture:
                # Reuse the same admitted fixture as the shipped-host regression.
                # Fixture data replaces operator snapshots, never the product host.
                os.environ['CG_QUALIFICATION_BIN_DIR'] = str(args.bin_dir.resolve())
                sys.path.insert(0, str(ROOT / 'tests/codex-local'))
                if args.shared_session_fixture:
                    from test_sessions import SharedSessions
                    from test_canonical_host import CanonicalHost
                    fixture = SharedSessions()
                    fixture.request_for = CanonicalHost.request_for.__get__(fixture)
                    fixture.cli_envelope = CanonicalHost.cli_envelope.__get__(fixture)
                else:
                    from test_canonical_host import CanonicalHost
                    fixture = CanonicalHost()
                fixture.setUp()
                if args.shared_session_fixture:
                    fixture.configure(consent=True)
                    fixture.add_sources()
                else:
                    fixture.prepare()
                launch, request = fixture.launch, fixture.request
                launch[launch.index('--client-name') + 1] = args.client_name
                launch[launch.index('--client-version') + 1] = client_version
                (bootstrap / 'request.json').write_text(json.dumps(request))
                report['scope'] = 'installed Codex -> shipped CG inspection and canonical operations'
                report['substitutions'] = ['Neutral admitted plan/rules/process/policy/projection snapshots and catalog; no application host substitution.']
                report['limitations'][0] = 'Shared-session lifecycle requires separate qualification; this run does not supply session services.'
                if args.shared_session_fixture:
                    report['scope'] = 'installed Codex -> shipped CG canonical and shared structured-session operations'
                    report['substitutions'].append('Disposable loopback PostgreSQL; registered verified context-artifact Intent and two neutral source alternatives.')
                    report['limitations'][0] = 'Only the registered structured context-artifact task is qualified; full EPIC-04 reconciliation remains separate.'
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
            report['installed_client']['native_binary'] = native_identity(app.process)
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
            if server['runtimeStatus'] != 'connected':
                raise ValueError('installed client did not discover CG')
            verify_catalog(server['tools'], args.shared_session_fixture)
            for major in ([1, 2] if args.shared_session_fixture else [1]):
                resource = app.call('mcpServer/resource/read', {'threadId': thread,
                    'server': 'cognitive_gateway', 'uri': f'cg://contracts/{major}.0/catalog'})
                if json.loads(resource['contents'][0]['text']) != json.loads(
                        (ROOT / f'schemas/codex/v{major}/catalog.json').read_text()):
                    raise ValueError('installed frozen catalog resource differs')
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
            if args.shared_session_fixture:
                def invoke_session(arguments):
                    response = app.call('mcpServer/tool/call', {'threadId': thread, 'server': 'cognitive_gateway',
                        'tool': 'cg_' + arguments['operation'].replace('.', '_') + '_v2', 'arguments': arguments})
                    from test_sessions import VALIDATORS
                    VALIDATORS['response'].validate(response['structuredContent'])
                    if response.get('isError', False) != (response['structuredContent']['status'] != 'ok'):
                        raise ValueError('MCP error flag differs from session envelope')
                    return response['structuredContent']

                def reconnect():
                    nonlocal app, thread
                    previous = app.transcript
                    report['cleanup'].append(app.close())
                    app = AppServer(codex, home)
                    app.transcript = previous
                    app.call('initialize', {'clientInfo': {'name': 'cg_qualification', 'version': '1.0'},
                                            'capabilities': {'experimentalApi': True}})
                    app.notify('initialized')
                    thread = app.call('thread/start', {'cwd': str(home), 'ephemeral': True,
                        'sandbox': 'read-only', 'approvalPolicy': 'never'})['thread']['id']
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        current = app.call('mcpServerStatus/list', {'threadId': thread})
                        connected = next(s for s in current['data'] if s['name'] == 'cognitive_gateway')
                        if connected['runtimeStatus'] == 'connected':
                            verify_catalog(connected['tools'], True)
                            return
                        if connected['runtimeStatus'] != 'starting':
                            break
                        time.sleep(0.1)
                    raise ValueError('installed reconnect failed')

                sessions = qualify_sessions(fixture, invoke_session, reconnect)
                report['session_checks'] = sessions['checks']
                report['session_cli_inspection_parity'] = True
                report['consent_issuer'] = 'trusted operator cg-local session.authority; never Codex approval'
                legacy = app.call('mcpServer/tool/call', {'threadId': thread, 'server': 'cognitive_gateway',
                    'tool': 'cg_session_inspect_v1', 'arguments': fixture.request_for('session.inspect', {
                        'session_id': sessions['completed']['result']['session']['session']})})
                if not legacy.get('isError') or legacy['structuredContent']['status'] != 'unsupported':
                    raise ValueError('frozen v1 session behavior changed')
                report['frozen_v1_session'] = 'unsupported'
                evidence = sessions['completed']['result']['session']['final_evidence']
                scope = request['scope']
                uri = (f"cg://workspaces/{scope['workspace_id']}/projects/{scope['project_id']}/"
                       f"bindings/{scope['binding_id']}/references/{evidence['id']}/{evidence['revision']}/{evidence['digest']}")
                resource = app.call('mcpServer/resource/read', {'threadId': thread,
                    'server': 'cognitive_gateway', 'uri': uri})
                receipt = json.loads(resource['contents'][0]['text'])
                from test_sessions import VALIDATORS
                VALIDATORS['resource'].validate(receipt)
                document = receipt['document']
                if (document['goal_outcome'] != 'SATISFIED' or not document['facts'] or not document['evidence']
                        or document['verification']['basis']['projection'] != fixture.config['basis']['projection']):
                    raise ValueError('independent evidence for registered artifact goal missing')
                (output / 'verified-evidence.json').write_text(json.dumps(receipt, indent=2) + '\n')
                # Current policy is loaded by a new real host; no client can override it.
                fixture.admission['mappings'][0]['canonical']['policy']['steps']['step-condition.0']['authorizations'] = {}
                fixture.save()
                reconnect()
                refused = invoke('context.compile', fixture.request_for('context.compile', {
                    'resolution': reference, 'projection': fixture.references['projection'],
                    'step_id': 'step-condition.0', 'candidates': []}))
                if refused['status'] != 'blocked' or refused['diagnostics'][0]['code'] != 'CG_CONSENT_REQUIRED':
                    raise ValueError('installed current-policy refusal missing')
                report['policy_denial'] = 'CG_CONSENT_REQUIRED'
                report['admission_denials'] = []
                for field, value in [('--client-name', 'unadmitted-client'), ('--client-version', '0.0.0')]:
                    # Connect the real client directly: the host, not the observer, must reject it.
                    previous = app.transcript
                    report['cleanup'].append(app.close())
                    bad_launch = list(launch)
                    bad_launch[bad_launch.index(field) + 1] = value
                    bad_config = '[mcp_servers.cognitive_gateway]\ncommand = ' + json.dumps(command) + '\n'
                    bad_config += 'args = ' + json.dumps(['-i', str((args.bin_dir / 'cg-mcp').resolve()), *bad_launch]) + '\nstartup_timeout_sec = 10\n'
                    (home / 'config.toml').write_text(bad_config)
                    app = AppServer(codex, home)
                    app.transcript = previous
                    app.call('initialize', {'clientInfo': {'name': 'cg_qualification', 'version': '1.0'},
                                            'capabilities': {'experimentalApi': True}})
                    app.notify('initialized')
                    thread = app.call('thread/start', {'cwd': str(home), 'ephemeral': True,
                        'sandbox': 'read-only', 'approvalPolicy': 'never'})['thread']['id']
                    deadline = time.monotonic() + 15
                    while True:
                        status = app.call('mcpServerStatus/list', {'threadId': thread})
                        refused = next(s for s in status['data'] if s['name'] == 'cognitive_gateway')
                        if refused['runtimeStatus'] != 'starting' or time.monotonic() >= deadline:
                            break
                        time.sleep(0.1)
                    if refused['runtimeStatus'] == 'connected' or refused['tools'] or refused['runtimeStatus'] == 'starting':
                        raise ValueError('wrong trusted admission accepted or unresolved')
                    report['admission_denials'].append({'field': field, 'runtime_status': refused['runtimeStatus']})
            report['initialize'] = json.loads(metadata.read_text())
            report['discovered_tools'] = len(server['tools'])
            report['canonical_response'] = canonical
            report['scope_denial'] = 'CG_SCOPE_DENIED'
            report['cli_parity'] = True
            report['status'] = 'QUALIFIED_INSTALLED_CANONICAL' if fixture else 'QUALIFIED_INSTALLED_INSPECTION'
            if args.shared_session_fixture:
                report['status'] = 'QUALIFIED_INSTALLED_SHARED_SESSIONS'
            # Retain only admitted fixture results and protocol metadata, no raw auth/config.
            retain_transcript(output, app.transcript)
    except Exception as error:
        if isinstance(error, (NotImplementedError, ImportError)):
            report['status'] = 'BLOCKED'
        if report['status'] not in ('NOT_RUN', 'BLOCKED'):
            report['status'] = 'FAIL'
        report['diagnostic'] = 'Installed-client qualification did not complete; no success inferred.'
        report['failure_type'] = type(error).__name__
    finally:
        if app:
            retain_transcript(output, app.transcript)
            report['cleanup'].append(app.close())
        if fixture:
            fixture.doCleanups()
        report['finished_at_unix'] = time.time()
        if report['status'].startswith('QUALIFIED_INSTALLED_'):
            unchanged = all((ROOT / name).is_file() and sha256(ROOT / name) == digest
                            for name, digest in report['source_sha256'].items())
            unchanged = unchanged and all(sha256((args.bin_dir / name).resolve()) == digest
                                          for name, digest in report['binary_sha256'].items())
            unchanged = unchanged and sha256(codex) == report['installed_client']['launcher_sha256']
            unchanged = unchanged and report['revision'] == subprocess.check_output(
                ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
            native = report['installed_client']['native_binary']
            unchanged = unchanged and sha256(Path(native['path'])) == native['sha256']
            unchanged = unchanged and all(c['elapsed_seconds'] < 15 and c['eof_exit_code'] == 0 and not c['forced']
                                          for c in report['cleanup'])
            if not unchanged:
                report['status'] = 'FAIL'
                report['diagnostic'] = 'Candidate sources or executables changed during qualification.'
            else:
                report['epic_04_status'] = 'NOT_ASSESSED'
        report['artifact_sha256'] = {path.name: sha256(path) for path in output.iterdir() if path.is_file()}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{report['status']}: {output / 'report.json'}")
    return 0 if report['status'].startswith('QUALIFIED_INSTALLED_') else 1


if __name__ == '__main__':
    if sys.argv[1:2] == ['--proxy-initialize']:
        sys.exit(proxy(sys.argv[2:]))
    sys.exit(main())
