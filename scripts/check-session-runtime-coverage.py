#!/usr/bin/env python3
"""Require >=95% measured coverage for the application and daemon session gates."""
import importlib.util
import json
from pathlib import Path
import sys

SPEC = importlib.util.spec_from_file_location(
    "session_coverage_gate", Path(__file__).with_name("check-local-mcp-coverage.py")
)
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)

APPLICATION_EXPECTED = tuple(
    f"crates/gateway-application/src/sessions/{name}.rs"
    for name in ("contracts", "admission", "authority", "journal", "coordinator", "verification", "boundary", "connector_execution", "connector_goals", "readback_storage", "resources")
) + (
    "crates/gateway-application/src/codex/ports.rs",
    "crates/gateway-application/src/connector_proposals.rs",
    "crates/gateway-application/src/connector_policy.rs",
    "crates/gateway-application/src/policy_application.rs",
    "crates/gateway-application/src/retained_observations.rs",
    "crates/gateway-application/src/closed_loop.rs",
)
DAEMON_EXPECTED = (
    "crates/gateway-daemon/src/session_store.rs",
    "crates/gateway-daemon/src/session_store/connector_policy_audit.rs",
    "crates/gateway-daemon/src/local_sessions.rs",
    "crates/gateway-daemon/src/local_sessions/capability_alias.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission/connector_source.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission/target_factory.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission/model_documents.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission/model_factory.rs",
    "crates/gateway-daemon/src/local_sessions/runtime_admission/source_context.rs",
    "crates/gateway-daemon/src/local_sessions/goal_verifier.rs",
    "crates/gateway-daemon/src/local_sessions/selector_association.rs",
    "crates/gateway-daemon/src/cognitive_store.rs",
    "crates/gateway-daemon/src/codex_workspace.rs",
    "crates/gateway-daemon/src/local_mcp/mod.rs",
    "crates/gateway-daemon/src/bin/cg-local.rs",
    "crates/gateway-daemon/src/invocation_host/session_tools.rs",
    "crates/gateway-daemon/src/mcp_client/session_budget.rs",
)


def check_data(report, expected):
    previous = GATE.EXPECTED
    try:
        GATE.EXPECTED = expected
        return GATE.check(report)
    finally:
        GATE.EXPECTED = previous


def check_report(path, expected):
    with open(path, encoding="utf-8") as source:
        return check_data(json.load(source), expected)


def check(application_path, daemon_path):
    return [
        *check_report(application_path, APPLICATION_EXPECTED),
        *check_report(daemon_path, DAEMON_EXPECTED),
    ]


def self_test():
    def report(paths, covered=95, count=100):
        return {"data": [{"files": [
            {"filename": f"/workspace/{path}", "summary": {"lines": {"count": count, "covered": covered}}}
            for path in paths
        ]}]}

    app = report(APPLICATION_EXPECTED)
    daemon = report(DAEMON_EXPECTED)
    assert len(check_data(app, APPLICATION_EXPECTED)) == len(APPLICATION_EXPECTED)
    assert len(check_data(daemon, DAEMON_EXPECTED)) == len(DAEMON_EXPECTED)
    invalid = report(APPLICATION_EXPECTED, 94)
    try:
        check_data(invalid, APPLICATION_EXPECTED)
    except ValueError:
        pass
    else:
        raise AssertionError("session coverage gate accepted a value below 95%")
    print("Session runtime coverage gate self-tests passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif len(sys.argv) == 3 and sys.argv[1] == "--application":
        report = sys.argv[2]
        print("\n".join(check_report(report, APPLICATION_EXPECTED)))
    elif len(sys.argv) == 3:
        print("\n".join(check(sys.argv[1], sys.argv[2])))
    else:
        raise SystemExit(
            "usage: check-session-runtime-coverage.py APP_REPORT DAEMON_REPORT "
            "| --application APP_REPORT | --self-test"
        )
