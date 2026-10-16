#!/usr/bin/env python3
"""Require 95% coverage for EPIC-05 task, resolution and epistemic contracts."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    'semantic_task_coverage', Path(__file__).with_name('check-local-mcp-coverage.py'))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = ('crates/gateway-domain/src/semantic_task.rs',
                 'crates/gateway-domain/src/semantic_resolution.rs',
                 'crates/gateway-domain/src/semantic_epistemic.rs',
                 'crates/gateway-domain/src/semantic_output.rs')

if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding='utf-8') as source:
            print('\n'.join(GATE.check(json.load(source))))
