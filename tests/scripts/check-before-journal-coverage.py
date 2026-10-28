#!/usr/bin/env python3
"""Require exact >=95% line coverage for #407 executable production owners."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    'before_journal_coverage_gate', Path(__file__).with_name('check-local-mcp-coverage.py'))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = (
    'crates/gateway-application/src/sessions/before_snapshot/mod.rs',
    'crates/gateway-application/src/sessions/before_snapshot/codec.rs',
    'crates/gateway-application/src/sessions/coordinator.rs',
    'crates/gateway-application/src/sessions/journal.rs',
    'crates/gateway-application/src/sessions/goal_observation.rs',
    'crates/gateway-application/src/sessions/scoped_snapshots/codec.rs',
    'crates/gateway-application/src/sessions/scoped_snapshots/mod.rs',
    'crates/gateway-daemon/src/local_sessions.rs',
    'crates/gateway-daemon/src/session_store.rs',
)

if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding='utf-8') as source:
            print('\n'.join(GATE.check(json.load(source))))
