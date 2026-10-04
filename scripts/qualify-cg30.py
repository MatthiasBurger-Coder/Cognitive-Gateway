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


def validate_extended(directory):
    ml = json.loads((directory / "epic03-ml-experiment.json").read_text())
    live = json.loads((directory / "epic03-live-release.json").read_text())
    workers = json.loads((directory / "epic03-durable-workers.json").read_text())
    container = json.loads((directory / "epic03-worker-container.json").read_text())
    require(all(r["status"] == "PASS" for r in (ml, live, workers, container)), "Extended runtime acceptance failed")
    require(ml["suite"] == "EPIC03-ML-v1" and ml["reproduced"] is True, "ML experiment is not reproducible")
    artifact = ml["model"]["artifact"]
    encoded = json.dumps(artifact, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    require(hashlib.sha256(encoded).hexdigest() == ml["model"]["artifact_digest"], "ML artifact digest mismatch")
    require(artifact["test_used_for_selection"] is False and ml["evaluation"]["test_used_for_selection"] is False
            and live["test_used_for_selection"] is False, "Final test data was used for tuning")
    require(ml["evaluation"]["status"] == "PASS" and ml["evaluation"]["checks"]
            and all(value is True for value in ml["evaluation"]["checks"].values()), "ML quality profile failed")
    require(ml["grid_trials"] > 0 and ml["random_trials"] > 0 and artifact["trials"], "Missing bounded search evidence")
    for trial in artifact["trials"]:
        require(trial["folds"] and all(f["fit"] and f["score"] and not set(f["fit"]) & set(f["score"])
                                     for f in trial["folds"]), "Cross-validation leakage")
    require(artifact["calibration"]["fit_ids"] and artifact["calibration"]["after"]["brier"] <= artifact["calibration"]["before"]["brier"], "Missing calibration evidence")
    require(ml["drift"]["action"] == "REEVALUATE_RETRAIN_OR_ROLLBACK" and ml["drift"]["authority_changed"] is False, "Missing governed drift proof")
    require(ml["performance"]["cpu_ns"] > 0 and ml["performance"]["peak_process_rss_bytes"] > 0
            and ml["performance"]["external_provider_calls"] == 0, "Missing real CPU/resource measurements")
    require(live["real_cpu_training"] is True and live["postgres_restart"] is True
            and live["inference_versions"] == [2, 3, 2] and live["revoked_qualification_refused"] is True
            and live["prior_comparison"] is True, "Missing durable model/inference rollback proof")
    require(live["metrics"]["cases"] == 24 and live["metrics"]["tp"] == 12 and live["metrics"]["tn"] == 12
            and live["metrics"]["fp"] == 0 and live["metrics"]["fn"] == 0, "Live classifier regression")
    for manifest in live["journal"]["manifests"]:
        candidate = manifest["training"]["candidate"]
        version = candidate["version"]
        require(version in (2, 3) and sha256(directory / f"epic03-cpu-model-v{version}.json") == candidate["artifact_digest"], "Retained trained model mismatch")
        require("sha256-" + sha256(directory / f"epic03-cpu-evaluation-v{version}.json") == manifest["evaluation"]["evidence"], "Retained CPU evaluation mismatch")
    first_digest = next(m["training"]["candidate"]["artifact_digest"] for m in live["journal"]["manifests"] if m["training"]["candidate"]["version"] == 2)
    successor = json.loads((directory / "epic03-cpu-model-v3.json").read_text())
    successor_evaluation = json.loads((directory / "epic03-cpu-evaluation-v3.json").read_text())
    require(successor["artifact"]["prior_artifact_digest"] == first_digest
            and successor_evaluation["prior"] is not None and successor_evaluation["checks"]["prior"] is True,
            "Missing exact predecessor comparison")
    require(len(live["journal"]["events"]) == 9 and len(live["journal"]["manifests"]) == 2
            and live["journal"]["events"][-1]["action"] == "ROLLBACK", "Incomplete model journal")
    require(workers["coordinator_restart"] is True and workers["late_results_fenced"] is True
            and workers["competing_coordinators_single_claim"] is True and workers["duplicate_commits"] == 0,
            "Missing durable worker consistency proof")
    require(container["authority_credentials"] is False and container["production_mounts"] is False
            and container["worker_uid"] == "10001:10001" and container["limits"]["NetworkMode"] == "none"
            and container["limits"]["ReadonlyRootfs"] is True and container["limits"]["Memory"] == 268435456
            and container["limits"]["NanoCpus"] == 1000000000 and container["limits"]["PidsLimit"] == 32
            and container["limits"]["CapDrop"] == ["ALL"], "Worker container isolation changed")
    coverage = json.loads((directory / "epic03-python-coverage.json").read_text())
    require(coverage["totals"]["num_statements"] > 0 and coverage["totals"]["percent_covered"] >= 95, "Python coverage below 95%")
    return {"ml_suite": ml["suite"], "live_classification": live["metrics"], "baseline": live["baseline"],
            "cpu_measurements": ml["performance"], "inference_versions": live["inference_versions"],
            "postgres_restart": True, "worker_consistency": "PASS", "container_image": container["image_digest"],
            "container_limits": container["limits"], "python_coverage": coverage["totals"]["percent_covered"]}


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
    required = {"cg30-qualification.json", "cg16-coverage.json", "cg23-evaluation.json", "cg24-promotion.json", "cg25-reflex.json", "cg27-fixture-benchmark.json", "cg28-learning.json", "cg29-fabric.json", "cg03-host.json", "epic03-ml-experiment.json", "epic03-live-release.json", "epic03-durable-workers.json", "epic03-worker-container.json", "epic03-python-coverage.json", "epic03-cpu-model-v2.json", "epic03-cpu-model-v3.json", "epic03-cpu-evaluation-v2.json", "epic03-cpu-evaluation-v3.json"}
    require(required <= artifacts.keys(), "Release evidence artifact missing")
    require(all((directory / p).is_file() and sha256(directory / p) == h for p, h in artifacts.items()), "Artifact digest mismatch")
    evidence = validate_metrics(json.loads((directory / "cg30-qualification.json").read_text()))
    return {"schema_version": 1, "release": "v0.3", "status": "QUALIFIED_REFERENCE_RUNTIME_SCOPE", "revision": summary["revision"],
            "summary_sha256": sha256(directory / "summary.json"), "suite": evidence["suite"], "classification": {k: v for k, v in evidence["classification"].items() if k != "cases"},
            "routing": {k: v for k, v in evidence["routing"].items() if k != "cases"},
            "performance": evidence["performance"], "extended_runtime": validate_extended(directory), "toolchain": summary["toolchain"],
            "limitations": ["Qualified deterministic/reflex fixture lifecycle plus real CPU classification, PostgreSQL journals and isolated container inference on this host. Production workloads, GPUs and external model providers require their own deployment measurements.", "Qualification does not publish a release."],
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
