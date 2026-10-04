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

ROOT = Path(__file__).resolve().parent.parent


def capture(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).rstrip("\n")


def source_fingerprint(output):
    paths = subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    return {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
            for path in sorted(set(paths)) if path and (ROOT / path).is_file()
            and not (ROOT / path).resolve().is_relative_to(output)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="New evidence directory (must not exist)")
    args = parser.parse_args()
    with _HOST.CognitiveTestHost() as host:
        return run(args, host)


def run(args, host):
    if args.output:
        output = args.output.resolve()
        output.mkdir(parents=True, exist_ok=False)
    else:
        parent = ROOT / "target/release-evidence"
        parent.mkdir(parents=True, exist_ok=True)
        output = Path(tempfile.mkdtemp(prefix="declarative-v0.1-", dir=parent))
    print(f"Evidence: {output}", flush=True)
    gates = json.loads((ROOT / "scripts/quality-gates.json").read_text())
    if (not isinstance(gates, list) or not gates
            or any(not isinstance(gate, dict) or set(gate) != {"name", "command"}
                   or any(not isinstance(value, str) or not value.strip() for value in gate.values())
                   for gate in gates)
            or len({gate["name"] for gate in gates}) != len(gates)):
        raise ValueError("Gate manifest must contain unique named, nonempty commands")
    report = {"schema_version": 1, "scope": "declarative-v0.1", "status": "RUNNING",
              "revision": capture("git", "rev-parse", "HEAD"),
              "worktree_status": capture("git", "status", "--porcelain"),
              "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "gates": [{**gate, "status": "NOT_RUN"} for gate in gates]}
    (output / "worktree.patch").write_bytes(subprocess.check_output(
        ["git", "diff", "HEAD", "--binary"], cwd=ROOT))
    # Include untracked source in the fingerprint as well as tracked source.
    report["source_sha256"] = source_fingerprint(output)
    env = os.environ.copy()
    env.update(host.environment)
    (output / "cg03-host.json").write_text(json.dumps(host.report, indent=2) + "\n")
    # Only the dedicated proof gate may export; workspace/coverage tests also run CG-12.
    env.pop("CG12_EXPORT_DIR", None)
    for name in ("CG03_LIVE_OUTPUT", "CG03_DURABLE_WORKER_OUTPUT"):
        env.pop(name, None)
    report["build_environment"] = {name: env[name] for name in
                                   ("CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUST_TEST_THREADS")
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
            for index, gate in enumerate(report["gates"]):
                gate["status"] = "RUNNING"
                (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
                print(f"[{index + 1}/{len(gates)}] {gate['name']}", flush=True)
                gate["log"] = f"{index + 1:02d}.log"
                with (output / gate["log"]).open("w") as log:
                    result = subprocess.run(["bash", "-euo", "pipefail", "-c", gate["command"]],
                                            cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
                gate["exit_code"] = result.returncode
                gate["status"] = "PASS" if result.returncode == 0 else "FAIL"
                if result.returncode:
                    raise RuntimeError(f"{gate['name']} failed; see {output / gate['log']}")
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
            report["artifact_sha256"] = {str(path.relative_to(output)): hashlib.sha256(path.read_bytes()).hexdigest()
                                         for path in sorted(output.rglob("*"))
                                         if path.is_file() and path.name != "summary.json"}
            (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['status']}: {output / 'summary.json'}", flush=True)
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
