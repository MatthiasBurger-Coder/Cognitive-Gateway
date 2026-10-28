#!/usr/bin/env python3
"""Enforce the unchanged 95% line gate for #403 executable production files."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    'observed_goal_coverage_gate', Path(__file__).with_name('check-local-mcp-coverage.py'))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = (
    'crates/gateway-application/src/closed_loop.rs',
    'crates/gateway-application/src/prompt_rendering.rs',
    'crates/gateway-application/src/retained_observations.rs',
    'crates/gateway-application/src/sessions/connector_goals.rs',
    'crates/gateway-application/src/sessions/coordinator.rs',
    'crates/gateway-application/src/sessions/goal_observation.rs',
    'crates/gateway-application/src/sessions/journal.rs',
    'crates/gateway-application/src/sessions/pre_execution_goal.rs',
    'crates/gateway-application/src/sessions/pre_execution_source.rs',
    'crates/gateway-application/src/sessions/resources.rs',
    'crates/gateway-application/src/sessions/verification.rs',
    'crates/gateway-daemon/src/cognitive_store.rs',
    'crates/gateway-daemon/src/local_sessions.rs',
    'crates/gateway-daemon/src/local_sessions/definition_json.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/claim.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/definitions.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/mod.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/read_association.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/source.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/target.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/verify.rs',
    'crates/gateway-daemon/src/mcp_client/credential_file.rs',
    'crates/gateway-daemon/src/mcp_client/host.rs',
    'crates/gateway-daemon/src/mcp_client/mapping.rs',
    'crates/gateway-daemon/src/mcp_client/mod.rs',
    'crates/gateway-daemon/src/mcp_client/normalization.rs',
    'crates/gateway-daemon/src/mcp_client/protocol.rs',
    'crates/gateway-daemon/src/mcp_client/scalar_quotes.rs',
    'crates/gateway-daemon/src/mcp_client/session_budget.rs',
    'crates/gateway-daemon/src/mcp_client/transport.rs',
    'crates/gateway-daemon/src/session_store.rs',
    'crates/gateway-daemon/src/session_store_observation_audit.rs',
)


def check(report):
    result = GATE.check(report)
    # One private cfg(test) post-discovery call allows assertions against the
    # actual native Source and opaque coordinator child. All hook bodies live in
    # a separate test file. Charge its one line as UNCOVERED, retaining the full
    # denominator, so that this fixture point cannot help production pass 95%.
    path = 'crates/gateway-daemon/src/local_sessions/observed_goal/target.rs'
    entry = next(f for data in report['data'] for f in data['files']
                 if f['filename'].replace('\\', '/').endswith('/' + path)
                 or f['filename'].replace('\\', '/') == path)
    lines = entry['summary']['lines']
    if 100 * (lines['covered'] - 1) < 95 * lines['count']:
        raise ValueError('Below 95% after conservative fixture-point charge: ' + path)
    result.append('Target fixture point charged as uncovered; production threshold retained')
    return result


def self_test():
    GATE.self_test()
    def report(covered):
        return {'data': [{'files': [
            {'filename': path, 'summary': {'lines': {'count': 100, 'covered': covered}}}
            for path in GATE.EXPECTED]}]}
    check(report(96))
    try:
        check(report(95))
    except ValueError:
        pass
    else:
        raise AssertionError('Fixture line incorrectly helped the production threshold')

if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        self_test()
    else:
        with open(sys.argv[1], encoding='utf-8') as source:
            print('\n'.join(check(json.load(source))))
