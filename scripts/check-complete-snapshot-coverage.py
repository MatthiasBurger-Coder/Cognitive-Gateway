#!/usr/bin/env python3
"""Require measured >=95% coverage for every executable #405 production file."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    'complete_snapshot_coverage_gate', Path(__file__).with_name('check-local-mcp-coverage.py'))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = tuple(
    'crates/gateway-application/src/sessions/scoped_snapshots/' + name + '.rs'
    for name in ('mod', 'validation', 'identity', 'codec'))
# types.rs contains declarations only: LLVM emits no executable line entry.
# Its definitions are compiled and exercised through the four owning modules;
# adding fixture functions there would manufacture an artificial denominator.

if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding='utf-8') as source:
            print('\n'.join(GATE.check(json.load(source))))
