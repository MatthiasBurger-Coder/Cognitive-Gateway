#!/usr/bin/env python3
"""Require measured 95% line coverage for each EPIC-05.10 production module."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    "completeness_coverage", Path(__file__).with_name("check-cli-coverage.py"))
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = (
    "crates/gateway-application/src/completeness.rs",
)

if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print("\n".join(GATE.check(json.load(source))))
