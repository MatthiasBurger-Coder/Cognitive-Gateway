#!/usr/bin/env python3
"""Require measured 95% coverage for the shared session runtime and changed host files."""
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
    for name in ("contracts", "admission", "authority", "journal", "coordinator", "verification", "boundary")
)
GATE.EXPECTED += (
    "crates/gateway-application/src/codex/ports.rs",
    "crates/gateway-daemon/src/session_store.rs",
    "crates/gateway-daemon/src/local_sessions.rs",
    "crates/gateway-daemon/src/cognitive_store.rs",
    "crates/gateway-daemon/src/codex_workspace.rs",
    "crates/gateway-daemon/src/local_mcp/mod.rs",
    "crates/gateway-daemon/src/bin/cg-local.rs",
)

if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        GATE.self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print("\n".join(GATE.check(json.load(source))))
