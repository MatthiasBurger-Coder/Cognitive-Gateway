"""Read-only acceptance reconciliation; never launches inference or changes sources."""
import datetime
import hashlib
import gzip
import re
import importlib.util
import json
from pathlib import Path
import subprocess

ROOT = Path.cwd().resolve()
if not (ROOT / 'scripts/qualify-invocation.py').is_file():
    raise SystemExit('Run this reviewer from the repository root.')


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def read(path):
    return json.loads(path.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def same_json(actual, expected):
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(same_json(actual[k], v) for k, v in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(same_json(a, b) for a, b in zip(actual, expected))
    return actual == expected


def main():
    q = module('fixture_qualification', ROOT / 'scripts/qualify-invocation.py')
    live = module('live_qualification', ROOT / 'scripts/qualify-invocation-live.py')
    manifest = q.validate_manifest(read(q.MANIFEST))
    dataset = read(ROOT / 'models/datasets/invocation-extraction-v2.json')
    checks = []

    def check(name, passed, detail=None):
        checks.append(dict(name=name, passed=bool(passed), detail=detail))

    component_paths = list((ROOT / 'target').glob('epic06-*final*/qualification.json'))
    live_paths = list(Path('/home/micro/.cache').glob('epic06-host-live-qualification-v*/live-qualification.json'))
    component_path = max(component_paths, key=lambda p: p.stat().st_mtime) if component_paths else None
    live_path = max(live_paths, key=lambda p: p.stat().st_mtime) if live_paths else None
    component = read(component_path) if component_path else {}
    recorded_live = read(live_path) if live_path else {}
    check('Full component qualification passes', component.get('status') == 'COMPONENT_PASS')
    if component_path:
        current = q.fingerprint(component_path.parent)
        previous = component.get('source_sha256', {})
        changed = sorted(p for p in current.keys() | previous.keys() if current.get(p) != previous.get(p))
        check('Full component source snapshot is current, including additions', not changed and not component.get('source_drift'), changed)
        check('Manifest is the qualified manifest', component.get('manifest_sha256') == digest(q.MANIFEST))
    gates = component.get('gates', [])
    check('Every required gate passes', {g['id'] for g in gates} == {g['id'] for g in manifest['gates']}
          and all(g.get('status') == 'PASS' and (not g.get('rust_tests') or g.get('tests_passed', 0) > 0) for g in gates))
    indexed = {c['id']: c for c in component.get('criteria', [])}
    check('All 40 current manifest criteria are verified', set(indexed) == {c['id'] for c in manifest['criteria']}
          and all(c.get('status') == 'VERIFIED' and c.get('tests_verified') is True for c in indexed.values()))
    if component_path:
        for gate in gates:
            path = component_path.parent / gate.get('log', 'absent')
            check('Retained gate log: ' + gate['id'], path.is_file() and digest(path) == gate.get('log_sha256'))

    review_path = ROOT / 'docs/evidence/EPIC-06-parent-acceptance-review.json'
    requirements_review = read(review_path) if review_path.is_file() else {}
    refresh = requirements_review.get('final_requirement_source_refresh', {})
    refreshed_source = ROOT / refresh.get('retained_snapshot', 'absent')
    check('Current requirement sources match the complete reviewed issue inventory',
          refresh.get('status') == 'VERIFIED_UNCHANGED' and refresh.get('changed_issue_bodies') == []
          and set(refresh.get('issues_checked', [])) == {178, *range(261, 271)} and refreshed_source.is_file())
    if refreshed_source.is_file():
        check('Current requirements snapshot hash is retained',
              hashlib.sha256(gzip.decompress(refreshed_source.read_bytes())).hexdigest() == refresh.get('snapshot_sha256'))
    mapped_children = [c for issue in requirements_review.get('child_criteria', []) for c in issue.get('criteria', [])]
    manifest_ids = {c['id'] for c in manifest['criteria']}
    check('All 40 epic criteria and all 56 child acceptance items remain mapped',
          {c['id'] for c in requirements_review.get('criteria', [])} == manifest_ids
          and len(mapped_children) == 56 and len({c['id'] for c in mapped_children}) == 56
          and all(c.get('mapped_criteria') and set(c['mapped_criteria']) <= manifest_ids for c in mapped_children))
    workspace = requirements_review.get('workspace_verification', {})
    archived_log = ROOT / workspace.get('retained_log', 'absent')
    workspace_report = ROOT / workspace.get('source_component_report', 'absent')
    check('Full workspace suite is retained and passing', workspace.get('status') == 'PASS'
          and workspace.get('tests_failed') == 0 and workspace.get('tests_passed', 0) > 0
          and archived_log.is_file() and workspace_report.is_file())
    if archived_log.is_file() and workspace_report.is_file():
        log = gzip.decompress(archived_log.read_bytes())
        groups = re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored', log.decode())
        check('Full workspace transcript matches recorded counts and hash',
              hashlib.sha256(log).hexdigest() == workspace.get('log_sha256') and bool(groups)
              and all(g[0] == 'ok' and int(g[2]) == 0 for g in groups)
              and sum(int(g[1]) for g in groups) == workspace.get('tests_passed')
              and sum(int(g[3]) for g in groups) == workspace.get('tests_ignored'))
        prior_sources = read(workspace_report).get('source_sha256', {})
        current_sources = component.get('source_sha256', {})
        workspace_drift = sorted(p for p in prior_sources.keys() | current_sources.keys()
                                 if prior_sources.get(p) != current_sources.get(p))
        check('Full workspace production, fixtures and Rust sources remain current', bool(prior_sources)
              and not workspace_drift, workspace_drift)

    check('Both real adapter paths have a passing live role report', recorded_live.get('status') == 'LIVE_ROLE_PASS'
          and recorded_live.get('evidence_kind') == 'live')
    check('Live dataset is the unchanged fixed dataset', recorded_live.get('dataset_sha256') == live.sha(live.compact(dataset)))
    host_binary = ROOT / 'target/debug/cg-invoke'
    check('Live host binary is the retained qualified binary', host_binary.is_file() and digest(host_binary) == recorded_live.get('binary_sha256'))
    check('Live source snapshot is current', recorded_live.get('source_sha256') == live.fingerprint()
          and not recorded_live.get('source_drift'))
    check('Component and live reports bind identical shared sources', bool(recorded_live.get('source_sha256'))
          and all(component.get('source_sha256', {}).get(p) == h for p, h in recorded_live.get('source_sha256', {}).items()))
    role_file = ROOT / 'crates/gateway-daemon/src/file_request_extraction_v1.txt'
    if role_file.is_file():
        check('Live fingerprint binds the registered instruction definition',
              recorded_live.get('source_sha256', {}).get(str(role_file.relative_to(ROOT))) == digest(role_file))
    adapters = recorded_live.get('adapters', [])
    check('Exactly the two requested live adapters are present', len(adapters) == 2
          and {a.get('adapter') for a in adapters} == {'local', 'chat-completions'})
    expected = {c['id']: c for c in dataset['cases']}
    for adapter in adapters:
        kind = adapter['adapter']
        decision = adapter.get('decision', {})
        check(kind + ': evaluator approves real live evidence', adapter.get('evaluator_exit_code') == 0
              and decision.get('eligible_for_promotion') is True and decision.get('gates_passed') is True
              and decision.get('evidence') == 'live' and decision.get('failed_metrics') == {}
              and decision.get('diagnostics') == [])
        samples = adapter.get('samples', [])
        check(kind + ': all ten fixed cases are present exactly once', len(samples) == len(expected)
              and {s.get('case') for s in samples} == set(expected))
        fixed_system = None
        fixed_generation = None
        for sample in samples:
            name = sample.get('case')
            frozen = expected.get(name, {}).get('expected')
            response = sample.get('response', {})
            check(kind + ': independently graded case ' + str(name), sample.get('exit_code') == 0
                  and response.get('status') == 'VERIFIED' and response.get('verification', {}).get('validity') == 'VALID'
                  and same_json(sample.get('expected'), frozen)
                  and same_json(response.get('parsed', {}).get('result'), frozen))
            receipt = response.get('adapter', {}).get('receipt', {})
            generation = receipt.get('generation', {})
            if fixed_generation is None:
                fixed_generation = generation.get('requested_profile_digest')
            installed = recorded_live.get('installed_profile', {})
            expected_actual_id = installed.get('model_id') if kind == 'local' else installed.get('runtime_model')
            check(kind + ': exact installed model and explicit generation receipt for ' + str(name),
                  receipt.get('actual_model_id') == expected_actual_id and bool(expected_actual_id)
                  and receipt.get('model', {}).get('id') == installed.get('model_id')
                  and receipt.get('model', {}).get('digest') == installed.get('artifact_digest', '').removeprefix('sha256:')
                  and generation.get('structured_output_mode') == 'JSON'
                  and generation.get('max_output_tokens') == 1024 and generation.get('unexposed_defaults') == []
                  and bool(fixed_generation) and generation.get('requested_profile_digest') == fixed_generation
                  and receipt.get('profile') == response.get('trace', {}).get('accounting', {}).get('count', {}).get('profile'))
            if decision.get('gates_passed') is True and live_path:
                journals = list((live_path.parent / (kind + '-' + name) / 'journal').glob('*.json'))
                check(kind + ': exactly one durable journal for ' + name, len(journals) == 1)
                if len(journals) == 1:
                    saved = read(journals[0]).get('records', {}).get('sample', {})
                    handoff = saved.get('ir', {}).get('prompt', {}).get('handoff', {})
                    wire_text = saved.get('rendered', {}).get('rendered', {}).get('payload', '')
                    try:
                        wire = json.loads(wire_text)
                        data = json.loads(wire['messages'][1]['content'])
                    except (ValueError, KeyError, IndexError, TypeError):
                        wire, data = {}, {}
                    messages = wire.get('messages', [])
                    system = messages[0].get('content') if len(messages) == 2 else None
                    if fixed_system is None:
                        fixed_system = system
                    check(kind + ': two-role bridge and fixed authority across cases for ' + name,
                          len(messages) == 2 and [m.get('role') for m in messages] == ['system', 'user']
                          and isinstance(system, str) and bool(system) and system == fixed_system)
                    check(kind + ': explicit JSON wire mode and output bound for ' + name,
                          (kind == 'local' and wire.get('structured_output_mode') == 'JSON' and wire.get('max_output_tokens') == 1024)
                          or (kind == 'chat-completions' and wire.get('response_format') == {'type': 'json_object'}
                              and wire.get('max_tokens') == 1024))
                    if role_file.is_file():
                        profile = response.get('instruction_profile', {})
                        check(kind + ': registered role definition and profile are bound for ' + name,
                              profile.get('id') == 'file-request-extraction-v1' and profile.get('revision') == '1'
                              and profile.get('digest') == digest(role_file)
                              and role_file.read_text() in (system or ''))
                    if role_file.is_file() or 'request' in data:
                        check(kind + ': request alias equals original frozen input for ' + name,
                              same_json(data.get('request'), expected.get(name, {}).get('input')))
                    check(kind + ': full IR and exact wire retained for ' + name,
                          bool(handoff) and bool(wire) and 'raw' in saved and 'ir' not in response and 'rendered' not in response)
                    check(kind + ': inline semantics preserved for ' + name,
                          all(key in handoff and same_json(data.get('semantic', {}).get(key), handoff[key])
                              for key in ('schema_version', 'task', 'context', 'evidence', 'examples', 'output')))
                    expected_schema = {'$schema': 'https://json-schema.org/draft/2020-12/schema',
                                       'type': 'object', 'additionalProperties': False,
                                       'properties': {k: {'type': 'boolean' if k == 'ambiguous' else 'string'}
                                                      for k in sorted(frozen)}, 'required': sorted(frozen)}
                    check(kind + ': strict schema and empty tool inventory preserved for ' + name,
                          same_json(data.get('output_schema'), expected_schema)
                          and same_json(data.get('tools'), saved.get('ir', {}).get('prompt', {}).get('tools'))
                          and data.get('tools') == {})
                    check(kind + ': audit receipt references resolve for ' + name,
                          all(key in handoff and data.get('audit_receipts', {}).get(key, {}).get('digest') == live.sha(live.compact(handoff[key]))
                              for key in ('bindings', 'planning', 'verification')))
                    check(kind + ': raw output and full payload hashes match trace for ' + name,
                          live.sha(bytes(saved.get('raw', []))) == response.get('trace', {}).get('replay', {}).get('output', {}).get('output_digest')
                          and live.sha(wire_text.encode()) == response.get('trace', {}).get('accounting', {}).get('count', {}).get('payload_digest'))
        probes = adapter.get('probes', {})
        check(kind + ': all negative and replay probes pass', set(probes) == {'provider_failure', 'cancellation', 'context_overflow', 'budget', 'semantic_rejection', 'replay'}
              and all(value is True for value in probes.values()))
        if live_path:
            profile_path = live_path.parent / (kind + '-profile.json')
            measurement_path = live_path.parent / (kind + '-measurements.json')
            evaluator = ROOT / 'target/debug/examples/qualify-invocation'
            check(kind + ': evaluator binary identity', evaluator.is_file() and digest(evaluator) == recorded_live.get('evaluator_sha256'))
            if profile_path.is_file() and measurement_path.is_file() and evaluator.is_file():
                profile, measurement = read(profile_path), read(measurement_path)
                check(kind + ': original thresholds and exact case digest', same_json(profile.get('thresholds'), live.thresholds(dataset))
                      and measurement.get('cases_digest') == live.sha(live.compact(samples)))
                result = subprocess.run([str(evaluator), str(profile_path), str(measurement_path)], capture_output=True, text=True, timeout=30)
                try:
                    repeated = json.loads(result.stdout)
                except ValueError:
                    repeated = None
                check(kind + ': current Rust evaluator reproduces exact decision', result.returncode == 0 and same_json(repeated, decision))
            else:
                check(kind + ': retained metric inputs exist', False)
    output = dict(schema_version='1.0', issue=178, recorded_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  status='ACCEPTED' if all(c['passed'] for c in checks) else 'NOT_COMPLETE',
                  scope='Read-only final acceptance against retained real execution reports; no new inference or model promotion',
                  component_report=str(component_path) if component_path else None, live_report=str(live_path) if live_path else None,
                  component_report_sha256=digest(component_path) if component_path else None,
                  live_report_sha256=digest(live_path) if live_path else None,
                  checks=checks, failed_checks=[c for c in checks if not c['passed']])
    Path('/tmp/epic06-final-audit.json').write_text(json.dumps(output, indent=2) + '\n')
    print(json.dumps({k: output[k] for k in ('status', 'component_report', 'live_report', 'failed_checks')}, indent=2))
    return 0 if output['status'] == 'ACCEPTED' else 1


if __name__ == '__main__':
    raise SystemExit(main())
