#!/usr/bin/env python3
"""Optional model operations and proposal-only inference; no authority state access."""
import argparse
import contextlib
import datetime
import fcntl
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import tempfile
import time
import urllib.error
import urllib.request

from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError

ROOT = Path(__file__).resolve().parents[2] if len(Path(__file__).resolve().parents) > 2 else Path('/')
SCHEMA = Path(os.environ.get('CG_MODEL_SCHEMA', ROOT / 'schemas/model-profile.schema.json'))
SUITE = Path(os.environ.get('CG_MODEL_SUITE', ROOT / 'models/suites/semantic-proposal-v1.json'))
MAX_BYTES = 2 * 1024 * 1024


class ModelError(Exception):
    def __init__(self, code):
        self.code = code
        super().__init__(code)


def digest(value):
    return 'sha256:' + hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def load(path):
    return json.loads(Path(path).read_text())


def validate(schema, value):
    try:
        Draft202012Validator.check_schema(schema)
    except SchemaError as error:
        raise ModelError('contract_invalid') from error
    if not Draft202012Validator(schema).is_valid(value):
        raise ModelError('contract_invalid')


class Runtime:
    def __init__(self, endpoint=None, timeout=None, acceleration=None):
        self.endpoint = endpoint or os.environ.get('CG_MODEL_RUNTIME_ENDPOINT', 'http://127.0.0.1:11434')
        self.timeout = float(timeout or os.environ.get('CG_MODEL_TIMEOUT', '180'))
        self.acceleration = acceleration or os.environ.get('CG_MODEL_ACCELERATION', 'cpu')
        if self.acceleration == 'gpu':
            self.acceleration = 'nvidia'
        if self.acceleration not in ('cpu', 'nvidia', 'amd', 'vulkan') or not 0 < self.timeout <= 3600:
            raise ModelError('configuration_invalid')

    def call(self, path, payload=None):
        data = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(self.endpoint + path, data=data, headers={'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                body = response.read(MAX_BYTES + 1)
                if len(body) > MAX_BYTES:
                    raise ModelError('runtime_response_too_large')
                value = json.loads(body)
                if not isinstance(value, dict) or 'error' in value:
                    raise ModelError('runtime_response_invalid')
                return value
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            raise ModelError('runtime_unavailable') from error
        except (ValueError, UnicodeError) as error:
            raise ModelError('runtime_response_invalid') from error

    def identity(self, profile):
        try:
            version = self.call('/api/version')['version']
            models = self.call('/api/tags')['models']
            entry = next((m for m in models if m['name'] == profile['runtime_model']), None)
            if entry is None:
                raise ModelError('model_missing')
            details = self.call('/api/show', {'model': profile['runtime_model']})
            artifact = entry['digest']
            if not artifact.startswith('sha256:'):
                artifact = 'sha256:' + artifact
            return {'artifact_digest': artifact, 'runtime_version': version,
                    'quantization': details['details']['quantization_level'],
                    'template_digest': digest(details.get('template', ''))}
        except (KeyError, TypeError, AttributeError) as error:
            raise ModelError('runtime_response_invalid') from error

    def check(self, profile):
        identity = self.identity(profile)
        if any(profile[key] != value for key, value in identity.items()):
            raise ModelError('provenance_changed')
        return identity

    def generate(self, profile, request, system, cold=False):
        if request.get('schema_version') != '1.0' or request.get('input_contract') not in profile['supported_input_contracts'] or request.get('output_contract') not in profile['supported_output_contracts']:
            raise ModelError('contract_unsupported')
        if not isinstance(request.get('prompt'), str) or not 0 < len(request['prompt']) <= 16000:
            raise ModelError('request_invalid')
        schema = request.get('output_schema')
        if not isinstance(schema, dict):
            raise ModelError('request_invalid')
        try:
            Draft202012Validator.check_schema(schema)
        except SchemaError as error:
            raise ModelError('request_invalid') from error
        if cold:
            self.call('/api/generate', {'model': profile['runtime_model'], 'keep_alive': 0})
        options = {'temperature': 0, 'seed': 0, 'num_ctx': profile['context_limit'], 'num_predict': 256}
        if self.acceleration == 'cpu':
            options['num_gpu'] = 0
        started = time.monotonic()
        result = self.call('/api/generate', {'model': profile['runtime_model'], 'prompt': json.dumps({'request': request['prompt']}),
                           'system': system, 'format': schema, 'stream': False, 'think': False,
                           'keep_alive': '5m', 'options': options})
        if not result.get('done') or result.get('done_reason') == 'length':
            raise ModelError('output_incomplete')
        try:
            proposal = json.loads(result['response'])
        except (ValueError, KeyError) as error:
            raise ModelError('output_invalid') from error
        validate(schema, proposal)
        return {'schema_version': '1.0', 'kind': 'proposal', 'model_id': profile['model_id'],
                'artifact_digest': profile['artifact_digest'], 'proposal': proposal,
                'metrics': {'latency_seconds': time.monotonic() - started,
                            'load_seconds': result.get('load_duration', 0) / 1e9,
                            'tokens_per_second': result.get('eval_count', 0) / max(result.get('eval_duration', 0) / 1e9, 1e-9)}}


class Registry:
    def __init__(self, path=None, runtime=None):
        self.path = Path(path or os.environ.get('CG_MODEL_REGISTRY', Path.home() / '.local/state/cognitive-gateway/models.json'))
        self.runtime = runtime or Runtime()

    @contextlib.contextmanager
    def locked(self, operation=False):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with contextlib.ExitStack() as stack:
            if operation:
                operations = stack.enter_context(self.path.with_suffix('.operations.lock').open('a'))
                fcntl.flock(operations, fcntl.LOCK_EX)
            lock = stack.enter_context(self.path.with_suffix('.lock').open('a'))
            self.lock = lock
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.state = load(self.path) if self.path.exists() else {'schema_version': '1.0', 'profiles': {}, 'aliases': {}, 'history': {}, 'audit': []}
            yield self

    @contextlib.contextmanager
    def runtime_work(self):
        # Long pulls and candidate benchmarks must leave the active alias usable.
        fcntl.flock(self.lock, fcntl.LOCK_UN)
        try:
            yield
        finally:
            fcntl.flock(self.lock, fcntl.LOCK_EX)
            if self.path.exists():
                self.state = load(self.path)

    def save(self, operation, model_id):
        self.state['audit'].append({'at': now(), 'operation': operation, 'model_id': model_id})
        fd, name = tempfile.mkstemp(dir=self.path.parent)
        try:
            with os.fdopen(fd, 'w') as stream:
                json.dump(self.state, stream, indent=2)
                stream.write('\n')
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(name, self.path)
            directory = os.open(self.path.parent, os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            if os.path.exists(name):
                os.unlink(name)

    def profile(self, model_id):
        if model_id not in self.state['profiles']:
            raise ModelError('profile_missing')
        return self.state['profiles'][model_id]['profile']

    def install(self, path):
        profile = load(path)
        validate(load(SCHEMA), profile)
        if profile['lifecycle'] != 'candidate' or profile['qualification_status'] != 'unqualified':
            raise ModelError('candidate_required')
        if profile['model_id'] in self.state['profiles']:
            raise ModelError('immutable_profile_exists')
        with self.runtime_work():
            self.runtime.call('/api/pull', {'model': profile['runtime_model'], 'stream': False})
            identity = self.runtime.identity(profile)
            for key, value in identity.items():
                if profile[key] is not None and profile[key] != value:
                    raise ModelError('provenance_changed')
                profile[key] = value
            # Snapshot the source tag into a per-profile runtime name. Later pulls
            # of a mutable upstream tag cannot overwrite the active artifact.
            profile['provenance']['revision'] = profile['artifact_digest']
            snapshot = 'cg-' + hashlib.sha256(profile['model_id'].encode()).hexdigest()[:24] + ':latest'
            self.runtime.call('/api/create', {'model': snapshot, 'from': profile['runtime_model'], 'stream': False})
            profile['runtime_model'] = snapshot
            profile.update(self.runtime.identity(profile))
        validate(load(SCHEMA), profile)
        if profile['model_id'] in self.state['profiles']:
            raise ModelError('immutable_profile_exists')
        self.state['profiles'][profile['model_id']] = {'profile': profile, 'evidence': None}
        self.save('install', profile['model_id'])
        return profile

    def binding(self, profile, suite):
        # Lifecycle transitions do not change qualification; inference inputs do.
        immutable = {k: v for k, v in profile.items() if k not in ('lifecycle', 'qualification_status')}
        return digest({'profile': immutable, 'suite': suite, 'acceleration': self.runtime.acceleration,
                       'profile_schema': load(SCHEMA),
                       'implementation': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()})

    def qualify(self, model_id):
        profile = self.profile(model_id)
        if profile['lifecycle'] != 'candidate':
            raise ModelError('candidate_required')
        suite = load(SUITE)
        if (suite['id'] != profile['qualification_suite'] or suite['id'] != profile['prompt_version']
                or suite['input_contract'] not in profile['supported_input_contracts']
                or suite['output_contract'] not in profile['supported_output_contracts']
                or not {'read', 'write', 'ambiguity', 'no-invention', 'injection'}.issubset({case['id'] for case in suite['cases']})
                or suite['limits']['max_latency_seconds'] <= 0
                or suite['limits']['min_tokens_per_second'] <= 0
                or suite['limits']['max_model_memory_bytes'] <= 0):
            raise ModelError('suite_mismatch')
        evidence = {'at': now(), 'binding': self.binding(profile, suite), 'passed': False,
                    'hardware': {'platform': platform.platform(), 'cpu': platform.processor(),
                                 'cpu_count': os.cpu_count(), 'cpuinfo': Path('/proc/cpuinfo').read_text(),
                                 'meminfo': Path('/proc/meminfo').read_text()},
                    'acceleration': self.runtime.acceleration, 'samples': []}
        # Persist failed status before invoking the runtime: interruption cannot retain an old pass.
        profile['qualification_status'] = 'failed'
        self.state['profiles'][model_id]['evidence'] = evidence
        self.save('qualification_start', model_id)
        try:
            with self.runtime_work():
                self.runtime.check(profile)
                # Transport failure must fail closed, independently of any active alias.
                try:
                    Runtime('http://127.0.0.1:1', timeout=0.1).call('/api/version')
                except ModelError as error:
                    if error.code != 'runtime_unavailable':
                        raise
                else:
                    raise ModelError('failure_gate_failed')
                evidence['failure_gate'] = 'runtime_unavailable'
                cases = [suite['cases'][0], *suite['cases']]
                for index, case in enumerate(cases):
                    request = {'schema_version': '1.0', 'prompt': case['prompt'], 'input_contract': suite['input_contract'],
                               'output_contract': suite['output_contract'], 'output_schema': suite['output_schema']}
                    result = self.runtime.generate(profile, request, suite['system'], cold=index == 0)
                    sample = {'case': case['id'], 'cold': index == 0, **result}
                    evidence['samples'].append(sample)
                    if result['proposal'] != case['expected']:
                        raise ModelError('semantic_gate_failed')
                    limits = suite['limits']
                    if result['metrics']['latency_seconds'] > limits['max_latency_seconds'] or result['metrics']['tokens_per_second'] < limits['min_tokens_per_second']:
                        raise ModelError('benchmark_gate_failed')
                running = self.runtime.call('/api/ps')['models']
                resident = next((m for m in running if m['name'] == profile['runtime_model']), None)
                if resident is None or resident['size'] > suite['limits']['max_model_memory_bytes']:
                    raise ModelError('memory_gate_failed')
                if self.runtime.acceleration == 'cpu' and resident.get('size_vram', 0) != 0:
                    raise ModelError('cpu_gate_failed')
                evidence['resident_memory_bytes'] = resident['size']
                evidence['vram_bytes'] = resident.get('size_vram', 0)
                evidence['runtime_identity'] = self.runtime.check(profile)
                evidence['passed'] = True
                profile['qualification_status'] = 'passed'
        except ModelError as error:
            evidence['error'] = error.code
            raise
        finally:
            self.state['profiles'][model_id] = {'profile': profile, 'evidence': evidence}
            self.save('qualification_pass' if evidence['passed'] else 'qualification_fail', model_id)
        return evidence

    def qualified(self, profile):
        evidence = self.state['profiles'][profile['model_id']]['evidence']
        if profile['lifecycle'] == 'disabled' or profile['qualification_status'] != 'passed' or not evidence or not evidence['passed'] or evidence['binding'] != self.binding(profile, load(SUITE)):
            raise ModelError('qualification_required')
        self.runtime.check(profile)

    def promote(self, model_id, rollback=False):
        profile = self.profile(model_id)
        self.qualified(profile)
        role = profile['role']
        previous = self.state['aliases'].get(role)
        if previous == model_id:
            raise ModelError('already_active')
        if previous:
            self.profile(previous)['lifecycle'] = 'deprecated'
            self.state['history'].setdefault(role, []).append(previous)
        profile['lifecycle'] = 'active'
        self.state['aliases'][role] = model_id
        self.save('rollback' if rollback else 'promote', model_id)
        return {'id': role, 'resolves_to': model_id}

    def rollback(self, role):
        history = self.state['history'].get(role, [])
        if not history:
            raise ModelError('rollback_missing')
        target = history[-1]
        # Keep history intact if any validation fails.
        self.qualified(self.profile(target))
        history.pop()
        return self.promote(target, rollback=True)

    def disable(self, model_id):
        profile = self.profile(model_id)
        if model_id in self.state['aliases'].values():
            raise ModelError('active_disable_forbidden')
        profile['lifecycle'] = 'disabled'
        self.save('disable', model_id)
        return profile

    def active(self, role):
        model_id = self.state['aliases'].get(role)
        if not model_id:
            raise ModelError('active_model_missing')
        profile = self.profile(model_id)
        if profile['lifecycle'] != 'active':
            raise ModelError('active_model_missing')
        self.qualified(profile)
        return profile

    def infer(self, request):
        profile = self.active(request.get('role'))
        suite = load(SUITE)
        if request.get('output_schema') != suite['output_schema']:
            raise ModelError('contract_unsupported')
        return self.runtime.generate(profile, request, suite['system'])


class Handler(BaseHTTPRequestHandler):
    def respond(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == '/health':
            self.respond(200, {'status': 'live', 'schema_version': '1.0'})
        elif self.path == '/ready':
            self.apply(lambda registry: {'status': 'ready', 'model_id': registry.active(os.environ.get('CG_MODEL_ROLE', 'semantic-interpreter'))['model_id']})
        else:
            self.respond(404, {'error': 'route_missing'})

    def apply(self, operation):
        try:
            with Registry().locked() as registry:
                self.respond(200, operation(registry))
        except ModelError as error:
            self.respond(503, {'schema_version': '1.0', 'error': error.code, 'fallback': 'deterministic-core'})
        except (ValueError, KeyError, TypeError, OSError) as error:
            self.respond(400, {'error': 'request_invalid'})

    def do_POST(self):
        if self.path != '/v1/infer':
            self.respond(404, {'error': 'route_missing'})
            return
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= MAX_BYTES:
                raise ValueError()
            self.connection.settimeout(5)
            request = json.loads(self.rfile.read(length))
            if not isinstance(request, dict):
                raise ValueError()
        except (ValueError, TimeoutError):
            self.respond(400, {'error': 'request_invalid'})
            return
        self.apply(lambda registry: registry.infer(request))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['install', 'test', 'promote', 'rollback', 'disable', 'inspect', 'serve'])
    parser.add_argument('target', nargs='?')
    args = parser.parse_args()
    if args.operation == 'serve':
        ThreadingHTTPServer(('0.0.0.0', 8091), Handler).serve_forever()
        return
    try:
        with Registry().locked(operation=True) as registry:
            if args.operation == 'inspect':
                result = registry.state
            elif args.operation == 'test':
                result = registry.qualify(args.target)
            else:
                result = getattr(registry, args.operation)(args.target)
            print(json.dumps(result, indent=2))
    except (ModelError, ValueError, KeyError, TypeError, OSError) as error:
        print(json.dumps({'error': error.code if isinstance(error, ModelError) else 'configuration_invalid'}))
        raise SystemExit(1)


if __name__ == '__main__':
    main()
