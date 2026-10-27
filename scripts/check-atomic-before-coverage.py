#!/usr/bin/env python3
"""Require exact >=95% line coverage for #408 executable production owners."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    'atomic_before_coverage_gate', Path(__file__).with_name('check-local-mcp-coverage.py'))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = (
    'crates/gateway-application/src/sessions/goal_observation.rs',
    'crates/gateway-application/src/sessions/coordinator.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/mod.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/definitions.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/read_association.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/world_definitions.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/atomic_association.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/atomic_capture.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/source.rs',
    'crates/gateway-daemon/src/local_sessions/observed_goal/target.rs',
)

if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding='utf-8') as source:
            print('\n'.join(GATE.check(json.load(source))))
