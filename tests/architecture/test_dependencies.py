"""Mutation tests for boundaries that text-only Cargo.toml scans can miss."""
import copy
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("dependencies", ROOT / "scripts/check-dependencies.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class DependencyTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {"workspace_members": list(guard.ALLOWED), "packages": [
            {"id": name, "name": name, "manifest_path": f"/repo/crates/{name}/Cargo.toml",
             "dependencies": []} for name in guard.ALLOWED]}

    def test_current_graph_and_all_dependency_kinds(self):
        for package in self.metadata["packages"]:
            for name in guard.ALLOWED[package["name"]]:
                package["dependencies"].append({"name": name, "source": "registry+crates.io",
                    **({"path": f"/repo/crates/{name}"} if name in guard.ALLOWED else {})})
        guard.check(self.metadata)
        for kind in [None, "dev", "build"]:
            for target in [None, 'cfg(windows)']:
                invalid = copy.deepcopy(self.metadata)
                invalid["packages"][0]["dependencies"].append(
                    {"name": "reqwest", "rename": "serde", "kind": kind, "target": target})
                with self.assertRaises(ValueError):
                    guard.check(invalid)

    def test_outward_and_cross_component_dependencies(self):
        for source, target in [("gateway-domain", "gateway-application"),
                               ("gateway-application", "gateway-daemon"),
                               ("gateway-policy", "gateway-registry"),
                               ("gateway-context", "mcp")]:
            invalid = copy.deepcopy(self.metadata)
            next(p for p in invalid["packages"] if p["name"] == source)["dependencies"] = [{"name": target}]
            with self.assertRaises(ValueError):
                guard.check(invalid)

    def test_unknown_member_and_substituted_core(self):
        self.metadata["packages"].pop()
        with self.assertRaises(ValueError):
            guard.check(self.metadata)
        self.setUp()
        application = next(p for p in self.metadata["packages"] if p["name"] == "gateway-application")
        application["dependencies"] = [{"name": "gateway-domain", "path": "/other/domain"}]
        with self.assertRaises(ValueError):
            guard.check(self.metadata)

    def test_external_source_substitution(self):
        for source in [{"path": "/tmp/serde"}, {"source": "git+https://example.test/serde"}]:
            self.metadata["packages"][0]["dependencies"] = [{"name": "serde", **source}]
            with self.assertRaises(ValueError):
                guard.check(self.metadata)
