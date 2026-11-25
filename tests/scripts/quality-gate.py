#!/usr/bin/env python3
"""Run every declarative v0.1 gate and retain reviewable, fail-closed evidence."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import importlib.util

_HOST_SPEC = importlib.util.spec_from_file_location("cognitive_test_host", Path(__file__).with_name("cognitive-test-host.py"))
_HOST = importlib.util.module_from_spec(_HOST_SPEC)
_HOST_SPEC.loader.exec_module(_HOST)

ROOT = Path(__file__).resolve().parents[2]
SCOPE = "declarative-v0.1"
# Each boundary starts a span. Keep artifact producers and consumers in one job.
SHARD_BOUNDARIES = {
    "Gate regression tests": "foundation",
    "EPIC-05.17/18 context planning, dynamic examples and canonical CG-10 handoff coverage": "application",
    # CG-27 prepares the model virtualenv consumed by EPIC-06 coverage gates.
    "CG-27 local model lifecycle and adapter": "models",
    "CG-28 governed offline learning and rollback evidence": "application",
    "EPIC-06.01 model contracts and generation negotiation coverage": "models",
    "EPIC-07.03 MCP client runtime, stdio conformance and coverage": "connectors",
    "EPIC-08.03.04 installed atomic Before capture and coverage": "models",
    "EPIC-07.10 GitHub reference connector and scoped evidence coverage": "connectors",
}
SHARDS = tuple(dict.fromkeys(SHARD_BOUNDARIES.values()))


def capture(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).rstrip("\n")


def source_fingerprint(output):
    paths = subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    return {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
            for path in sorted(set(paths)) if path and (ROOT / path).is_file()
            and not (ROOT / path).resolve().is_relative_to(output)}


def load_gates():
    gates = json.loads((ROOT / "tests/scripts/quality-gates.json").read_text())
    if (not isinstance(gates, list) or not gates
            or any(not isinstance(gate, dict) or set(gate) != {"name", "command"}
                   or any(not isinstance(value, str) or not value.strip() for value in gate.values())
                   for gate in gates)
            or len({gate["name"] for gate in gates}) != len(gates)):
        raise ValueError("Gate manifest must contain unique named, nonempty commands")
    return gates


def shard_plan(gates):
    names = [gate["name"] for gate in gates]
    boundaries = list(SHARD_BOUNDARIES)
    positions = [names.index(name) for name in boundaries]
    if positions != sorted(positions) or positions[0] != 0:
        raise ValueError("Quality shard boundaries must follow manifest order and cover its first gate")
    plan = {shard: [] for shard in SHARDS}
    for name, start, end in zip(boundaries, positions, positions[1:] + [len(gates)]):
        plan[SHARD_BOUNDARIES[name]].extend(range(start, end))
    return plan


def artifact_hashes(directory):
    return {path.relative_to(directory).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.rglob("*"))
            if path.is_file() and path != directory / "summary.json"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def collect(parts, output):
    gates = load_gates()
    plan = shard_plan(gates)
    revision = capture("git", "rev-parse", "HEAD")
    worktree = capture("git", "status", "--porcelain")
    sources = source_fingerprint(output)
    reports = {}
    files = {}
    results = [None] * len(gates)
    require({path.name for path in parts.iterdir()} == {f"quality-shard-{shard}" for shard in plan}, "Missing or unexpected quality shard")
    for shard, indices in plan.items():
        directory = parts / f"quality-shard-{shard}"
        report = json.loads((directory / "summary.json").read_text())
        require(report.get("schema_version") == 1 and report.get("scope") == f"{SCOPE}-shard"
                and report.get("shard") == shard and report.get("status") == "PASS",
                f"{shard}: quality shard did not pass")
        require(report["revision"] == revision and report["source_sha256"] == sources
                and report["worktree_status"] == worktree, f"{shard}: source binding mismatch")
        require(not reports or report["toolchain"] == next(iter(reports.values()))["toolchain"],
                f"{shard}: toolchain mismatch")
        require([{key: result[key] for key in ("name", "command")} for result in report["gates"]]
                == [gates[index] for index in indices], f"{shard}: gate inventory mismatch")
        artifacts = artifact_hashes(directory)
        require(artifacts == report["artifact_sha256"], f"{shard}: artifact digest mismatch")
        for index, result in zip(indices, report["gates"]):
            log = f"{index + 1:02d}"
            require(result["status"] == "PASS"
                    and type(result.get("exit_code")) is int and result["exit_code"] == 0
                    and type(result.get("cleanup_exit_code")) is int and result["cleanup_exit_code"] == 0
                    and result["log"] == f"{log}.log" and result["log"] in artifacts
                    and result["cleanup_log"] == f"{log}-cleanup.log" and result["cleanup_log"] in artifacts,
                    f"{shard}: missing successful result or logs for {gates[index]['name']}")
            results[index] = result
        require({"worktree.patch", "cg03-host.json"} <= artifacts.keys(), f"{shard}: missing host or worktree evidence")
        for name, digest in artifacts.items():
            destination = name
            if name == "cg03-host.json" and shard != "application":
                destination = f"hosts/{shard}.json"
            if destination in files:
                require(destination == "worktree.patch" and digest == files[destination][1],
                        f"{shard}: conflicting artifact {destination}")
            files[destination] = (directory / name, digest)
        reports[shard] = report

    output.mkdir(parents=True, exist_ok=False)
    for name, (source, _) in files.items():
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    hashes = artifact_hashes(output)
    require(hashes == {name: digest for name, (_, digest) in files.items()}, "Combined artifact digest mismatch")
    require(source_fingerprint(output) == sources and capture("git", "rev-parse", "HEAD") == revision
            and capture("git", "status", "--porcelain") == worktree, "Source changed during collection")
    summary = {
        "schema_version": 1, "scope": SCOPE, "status": "PASS",
        "revision": revision, "worktree_status": worktree, "source_sha256": sources,
        "toolchain": next(iter(reports.values()))["toolchain"],
        "started_at": min(part["started_at"] for part in reports.values()),
        "finished_at": max(part["finished_at"] for part in reports.values()),
        "gates": results, "artifact_sha256": hashes,
        "shards": {shard: {key: part[key] for key in ("build_environment", "started_at", "finished_at")}
                   for shard, part in reports.items()},
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"PASS: {len(results)} gates from {len(reports)} shards; {output / 'summary.json'}")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="New evidence directory (must not exist)")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--shard", choices=SHARDS, help="Run one CI shard")
    mode.add_argument("--collect", type=Path, help="Validate and combine all CI shard evidence")
    args = parser.parse_args()
    if args.collect:
        if not args.output:
            parser.error("--collect requires --output")
        return collect(args.collect.resolve(), args.output.resolve())
    with _HOST.CognitiveTestHost() as host:
        return run(args, host)


def run(args, host):
    if args.output:
        output = args.output.resolve()
        output.mkdir(parents=True, exist_ok=False)
    else:
        parent = ROOT / "target/release-evidence"
        parent.mkdir(parents=True, exist_ok=True)
        output = Path(tempfile.mkdtemp(prefix=f"{SCOPE}-", dir=parent))
    print(f"Evidence: {output}", flush=True)
    gates = load_gates()
    diagnostic_tail = os.environ.get("CG_415_DIAGNOSTIC_TAIL") == "1"
    if args.shard:
        require(not diagnostic_tail, "Diagnostic tail cannot qualify a CI shard")
        indices = shard_plan(gates)[args.shard]
        scope = f"{SCOPE}-shard"
    elif diagnostic_tail:
        indices = [13, 26, 27, *range(37, len(gates))]
        scope = f"{SCOPE}-diagnostic-tail"
    else:
        indices = range(len(gates))
        scope = SCOPE
    total = len(gates)
    report = {"schema_version": 1, "scope": scope, "status": "RUNNING",
              "revision": capture("git", "rev-parse", "HEAD"),
              "worktree_status": capture("git", "status", "--porcelain"),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "gates": [{**gates[index], "status": "NOT_RUN"} for index in indices]}
    if args.shard:
        report["shard"] = args.shard
    (output / "worktree.patch").write_bytes(subprocess.check_output(
        ["git", "diff", "HEAD", "--binary"], cwd=ROOT))
    # Include untracked source in the fingerprint as well as tracked source.
    report["source_sha256"] = source_fingerprint(output)
    env = os.environ.copy()
    env.pop("CG_415_DIAGNOSTIC_TAIL", None)
    env.update(host.environment)
    target = Path(env.get("CARGO_TARGET_DIR") or "target")
    coverage_target = Path(env.get("CARGO_LLVM_COV_TARGET_DIR") or target / "llvm-cov-target")
    if not coverage_target.is_absolute():
        coverage_target = ROOT / coverage_target
    env["CARGO_LLVM_COV_TARGET_DIR"] = str(coverage_target)
    (output / "cg03-host.json").write_text(json.dumps(host.report, indent=2) + "\n")
    # Only the dedicated proof gate may export; workspace/coverage tests also run CG-12.
    env.pop("CG12_EXPORT_DIR", None)
    for name in ("CG03_LIVE_OUTPUT", "CG03_DURABLE_WORKER_OUTPUT"):
        env.pop(name, None)
    report["build_environment"] = {name: env[name] for name in
                                   ("CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "CARGO_LLVM_COV_TARGET_DIR", "RUST_TEST_THREADS")
                                   if name in env}
    with tempfile.TemporaryDirectory(prefix="cg13-") as temporary:
        env.update(RUNNER_TEMP=temporary, EVIDENCE_DIR=str(output), CARGO_TERM_COLOR="never")
        env["PATH"] = str(Path(temporary) / "cg-registry/bin") + os.pathsep + env["PATH"]
        # The proof stays outside the checkout during execution and is copied afterwards.
        try:
            report["toolchain"] = {tool: capture(*command) for tool, command in {
                "rustc": ["rustc", "--version"], "cargo": ["cargo", "--version"],
                "coverage": ["cargo", "llvm-cov", "--version"],
                "python": ["python3", "--version"]}.items()}
            for index, gate in zip(indices, report["gates"]):
                gate["status"] = "RUNNING"
                (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
                print(f"[{index + 1}/{total}] {gate['name']}", flush=True)
                gate["log"] = f"{index + 1:02d}.log"
                with (output / gate["log"]).open("w") as log:
                    result = subprocess.run(["bash", "-euo", "pipefail", "-c", gate["command"]],
                                            cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
                gate["exit_code"] = result.returncode
                gate["status"] = "PASS" if result.returncode == 0 else "FAIL"
                if result.returncode:
                    raise RuntimeError(f"{gate['name']} failed; see {output / gate['log']}")
                # Reports are durable evidence; successful builds are disposable.
                # One cache bounds disk use across the many isolated coverage epochs.
                gate["cleanup_log"] = f"{index + 1:02d}-cleanup.log"
                with (output / gate["cleanup_log"]).open("w") as log:
                    cleanup = subprocess.run(["cargo", "llvm-cov", "clean", "--workspace"],
                                             cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
                gate["cleanup_exit_code"] = cleanup.returncode
                if cleanup.returncode:
                    raise RuntimeError(f"Coverage cache cleanup after {gate['name']} failed; "
                                       f"see {output / gate['cleanup_log']}")
            if (source_fingerprint(output) != report["source_sha256"]
                    or capture("git", "rev-parse", "HEAD") != report["revision"]):
                raise RuntimeError("Source changed during the gate; rerun against a stable checkout")
            report["status"] = "PASS"
        except (OSError, subprocess.SubprocessError, RuntimeError, KeyboardInterrupt) as error:
            report["status"] = "FAIL"
            report["error"] = str(error)
            print(str(error), flush=True)
        finally:
            proof = Path(temporary) / "cg12-proof"
            if proof.exists():
                shutil.copytree(proof, output / "external-project-proof")
            report["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            report["artifact_sha256"] = artifact_hashes(output)
            (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['status']}: {output / 'summary.json'}", flush=True)
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
