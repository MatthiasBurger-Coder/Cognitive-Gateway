#!/usr/bin/env python3
"""Measure the #272 shared-contract prerequisite separately from #294 runtime."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    "session_coverage_gate", Path(__file__).with_name("check-local-mcp-coverage.py")
)
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
GATE.EXPECTED = tuple(
    f"crates/gateway-application/src/sessions/{name}.rs"
    for name in ("contracts", "admission", "authority", "journal")
)

if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print("\n".join(GATE.check(json.load(source))))
