#!/usr/bin/env python3
"""Validate CG-30 measurements or qualify a clean revision-bound v0.3 gate bundle."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TEST = "reflex_cases::cg30::cognitive_runtime_release_qualification"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def count(value):
    require(type(value) is int and value >= 0, "Missing or invalid measurement count")
    return value


def validate_metrics(bundle):
    evidence = bundle["evidence"]
    encoded = json.dumps(evidence, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    require(hashlib.sha256(encoded).hexdigest() == bundle["payload_sha256"], "CG-30 payload digest mismatch")
    policy = json.loads((ROOT / "tests/fixtures/epic03-v0.3/policy.json").read_text())
    require(evidence["schema_version"] == 1 and evidence["suite"] == policy["suite"], "Unknown CG-30 suite")
    require(evidence["policy"] == policy and evidence["fixture_only"] is True, "Changed policy or evidence basis")
    metrics = evidence["classification"]
    tp, tn, fp, fn = [count(metrics[key]) for key in ("true_positive", "true_negative", "false_positive", "false_negative")]
    require(tp + fn == policy["classification"]["positive_cases"] and tn + fp == policy["classification"]["negative_cases"], "Classification denominator mismatch")
    require(fp <= policy["classification"]["max_false_positive"] and fn <= policy["classification"]["max_false_negative"], "Reflex regression")
    require(metrics["false_positive_denominator"] == tn + fp and metrics["false_negative_denominator"] == tp + fn, "Missing classification denominator")
    require(metrics["false_positive_rate"] == fp / (tn + fp) and metrics["false_negative_rate"] == fn / (tp + fn), "Classification rate mismatch")
    rows = metrics["cases"]
    expected = {"exact", "inactive", "novel-superset", "different-scope", "ambiguous", "missing-evidence", "stale-evidence", "conflicting-evidence", "policy-bypass", "process-bypass", "blocker", "future-evidence"}
    require(len(rows) == len(expected) and {r["id"] for r in rows} == expected, "Missing classification scenario")
    for row in rows:
        positive = row["id"] == "exact"
        require(row["expected_activation"] is positive and count(row["runtime_calls"]) == int(positive), "Unsafe classification dispatch")
        require(row["result"]["dispatched"] == row["runtime_calls"], "Dispatch accounting mismatch")
        require(row["result"]["disposition"] == ("SUCCESS" if positive else "FULL_COGNITIVE_PATH"), "Unsafe fallback")
    routing = evidence["routing"]
    require(count(routing["total"]) == 10 and len(routing["cases"]) == 10, "Missing routing cases")
    require(count(routing["correct"]) / routing["total"] >= policy["routing"]["min_correct_fraction"], "Routing regression")
    require(count(routing["constraint_violations"]) <= policy["routing"]["max_constraint_violations"], "Routing constraint violation")
    for row in routing["cases"]:
        explanation = row["explanation"]
        selected, request = explanation["selected"], explanation["request"]
        require((selected["id"] if selected else None) == row["expected"], "Wrong routing selection")
        if selected:
            require(selected["available"] is True and selected["qualified"] is True, "Unavailable/unqualified route")
            require(count(selected["cost"]) <= request["max_cost"] and count(selected["latency_ms"]) <= request["max_latency_ms"], "Routing bound exceeded")
    routed = evidence["routing_execution"]
    expected_routing = {"model-failure-fallback": ("SUCCESS", 2),
                        "policy-revoked-before-fallback": ("PROCESS_OR_POLICY_DENIED", 1),
                        "all-models-unavailable": ("NO_COMPATIBLE_ROUTE", 0),
                        "model-identity-mismatch": ("INVALID_REPORT", 1)}
    require(len(routed) == len(expected_routing) and {r["id"] for r in routed} == expected_routing.keys(), "Missing model fallback proof")
    for row in routed:
        disposition, calls = expected_routing[row["id"]]
        telemetry = row["telemetry"]
        require(telemetry["disposition"] == disposition and row["runtime_calls"] == calls, "Unsafe model fallback")
        require(len(telemetry["attempts"]) == calls and len(telemetry["execution_provenance"]) == calls, "Missing model provenance")
        require(telemetry["consumed_cost"] == calls and telemetry["consumed_latency_ms"] == calls * 10, "Fallback budget accounting changed")
    failures = evidence["failures"]["cases"]
    require(len(failures) == 5 and {r["id"] for r in failures} == {"worker-hard-failure", "worker-retry-exhaustion", "reused-verification", "outcome-store-failure", "reservation-conflict"}, "Missing failure injection")
    for row in failures:
        require(row["result"]["disposition"] != "SUCCESS" and row["duplicate"]["failure"] == "REGISTRY_UNAVAILABLE", "Failure/duplicate falsely succeeded")
        require(count(row["runtime_calls"]) <= policy["failure_injection"]["max_retry_dispatches"], "Unbounded worker retry")
    require(count(evidence["performance"]["model_calls"]) <= policy["performance"]["max_model_calls"], "Unexpected model invocation")
    require(evidence["patterns"]["metrics"]["candidate_count"] == 1, "Missing detected candidate")
    proofs = evidence["reflex_proofs"]
    require(len(proofs) == 3 and [p["procedure"]["version"] for p in proofs] == [1, 2, 1], "Missing supersession/rollback proof")
    for proof in proofs:
        require(proof["disposition"] == "SUCCESS" and proof["failure"] is None, "Failed lifecycle proof")
        stages = {event["stage"] for event in proof["trace"]}
        require({"MATCH_ACTIVE", "APPLICABILITY_AND_EVIDENCE", "PROCESS_POLICY_COMPILED", "DISPATCH", "VERIFIED", "OUTCOME_RECORDED"} <= stages, "Incomplete provenance")
    return evidence


def qualify(directory):
    summary = json.loads((directory / "summary.json").read_text())
    require(summary["status"] == "PASS" and not summary["worktree_status"], "Release needs a passing clean-commit bundle")
    manifest = json.loads((ROOT / "scripts/quality-gates.json").read_text())
    require([{key: gate[key] for key in ("name", "command")} for gate in summary["gates"]] == manifest, "Gate manifest differs from candidate")
    require(all(gate["status"] == "PASS" and gate["exit_code"] == 0 for gate in summary["gates"]), "Not every established gate passed")
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip() == summary["revision"], "Candidate revision changed")
    require(not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip(), "Candidate worktree is dirty")
    sources = summary["source_sha256"]
    require(sources and all((ROOT / p).is_file() and sha256(ROOT / p) == h for p, h in sources.items()), "Source digest changed")
    artifacts = summary["artifact_sha256"]
    required = {"cg30-qualification.json", "cg16-coverage.json", "cg23-evaluation.json", "cg24-promotion.json", "cg25-reflex.json", "cg27-fixture-benchmark.json", "cg28-learning.json"}
    require(required <= artifacts.keys(), "Release evidence artifact missing")
    require(all((directory / p).is_file() and sha256(directory / p) == h for p, h in artifacts.items()), "Artifact digest mismatch")
    evidence = validate_metrics(json.loads((directory / "cg30-qualification.json").read_text()))
    return {"schema_version": 1, "release": "v0.3", "status": "QUALIFIED_FIXTURE_SCOPE", "revision": summary["revision"],
            "summary_sha256": sha256(directory / "summary.json"), "suite": evidence["suite"], "classification": {k: v for k, v in evidence["classification"].items() if k != "cases"},
            "routing": {k: v for k, v in evidence["routing"].items() if k != "cases"},
            "performance": evidence["performance"], "toolchain": summary["toolchain"],
            "limitations": ["Provider-free fixture acceptance; hardware latency, memory and monetary cost require separate deployment measurements.", "Qualification does not publish a release."],
            "artifacts": artifacts}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--metrics", type=Path)
    group.add_argument("--bundle", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.metrics:
            validate_metrics(json.loads(args.metrics.read_text()))
            print("CG-30 fixture metrics PASS")
        else:
            report = qualify(args.bundle.resolve())
            if args.output:
                with args.output.open("x") as output:
                    output.write(json.dumps(report, indent=2) + "\n")
            else:
                print(json.dumps(report, indent=2))
        return 0
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, ZeroDivisionError) as error:
        print(f"CG-30 qualification FAIL: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
