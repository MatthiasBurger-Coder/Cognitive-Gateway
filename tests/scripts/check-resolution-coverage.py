#!/usr/bin/env python3
"""Require at least 95% measured line coverage for every CG-08 resolver file."""
import importlib.util
import json
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("coverage_gate", root / "tests/scripts/check-cli-coverage.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
gate.EXPECTED = tuple(path.relative_to(root).as_posix() for path in sorted(
    (root / "crates/gateway-application/src").glob("resolution*.rs")))
if not gate.EXPECTED:
    raise ValueError("No resolver production files found")
if sys.argv[1:] == ["--self-test"]:
    gate.self_test()
else:
    with open(sys.argv[1], encoding="utf-8") as source:
        print("\n".join(gate.check(json.load(source))))
