#!/usr/bin/env python3
"""Check Cargo's normalized dependencies, including aliases, targets and dev/build."""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOWED = {
    "gateway-domain": {"serde", "serde_json", "sha2"},
    "gateway-application": {"gateway-domain", "gateway-context", "gateway-process",
                            "gateway-policy", "gateway-registry", "sha2", "serde", "serde_json"},
    "gateway-context": {"gateway-domain", "serde", "serde_json"},
    "gateway-process": {"gateway-domain", "serde", "serde_json", "sha2"},
    "gateway-policy": {"gateway-domain", "serde", "serde_json"},
    "gateway-registry": {"gateway-domain", "serde_json"},
    "gateway-workflow": {"gateway-domain"},
    "gateway-daemon": {"gateway-domain", "gateway-application", "gateway-context",
                       "gateway-process", "gateway-policy", "gateway-registry",
                       "gateway-workflow", "serde", "serde_json", "sha2", "postgres"},
}


def check(metadata):
    packages = {p["name"]: p for p in metadata["packages"]
                if p["id"] in metadata["workspace_members"]}
    if packages.keys() != ALLOWED.keys():
        raise ValueError("Workspace membership changed; review the architecture graph")
    graph = {}
    for name, package in packages.items():
        graph[name] = []
        for dependency in package["dependencies"]:
            target = dependency["name"]
            if target not in ALLOWED[name]:
                raise ValueError(f"Forbidden dependency: {name} -> {target}")
            if target in packages:
                expected = Path(packages[target]["manifest_path"]).parent.resolve()
                if Path(dependency.get("path", "")).resolve() != expected:
                    raise ValueError(f"Core dependency must use workspace source: {name} -> {target}")
                graph[name].append(target)
            elif dependency.get("path") or (dependency.get("source") or "").startswith("git+"):
                raise ValueError(f"Unreviewed dependency source: {name} -> {target}")
    def visit(name, active):
        if name in active:
            raise ValueError(f"Dependency cycle: {' -> '.join((*active, name))}")
        for target in graph[name]:
            visit(target, (*active, name))
    for name in graph:
        visit(name, ())


if __name__ == "__main__":
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"], cwd=ROOT))
    check(metadata)
    print("Cargo dependency graph passed (normal, dev, build and target dependencies)")
