"""EPIC-07.01 core boundary checks, including hidden dependency mutations."""
import copy
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("connector_dependencies", ROOT / "tests/scripts/check-dependencies.py")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class ConnectorArchitectureTests(unittest.TestCase):
    def test_public_contract_has_no_wire_provider_or_transport_types(self):
        source = (ROOT / "crates/gateway-application/src/connectors.rs").read_text()
        for forbidden in ("serde_json::Value", "rmcp::", "mcp::", "octocrab::",
                          "reqwest::", "std::net", "std::process", "tokio::",
                          "pub token:", "pub credential:", "pub url:", "pub uri:"):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, source)

    def test_provider_dependencies_cannot_hide_behind_alias_target_or_kind(self):
        metadata = {"workspace_members": list(guard.ALLOWED), "packages": [
            {"id": name, "name": name, "manifest_path": f"/repo/crates/{name}/Cargo.toml",
             "dependencies": []} for name in guard.ALLOWED]}
        guard.check(metadata)
        for owner in ("gateway-domain", "gateway-application"):
            for provider in ("rmcp", "octocrab", "reqwest", "github-mcp-server"):
                for kind in (None, "dev", "build"):
                    for target in (None, "cfg(windows)"):
                        with self.subTest(owner=owner, provider=provider, kind=kind, target=target):
                            candidate = copy.deepcopy(metadata)
                            package = next(p for p in candidate["packages"] if p["name"] == owner)
                            package["dependencies"].append({"name": provider, "rename": "serde",
                                                            "kind": kind, "target": target})
                            with self.assertRaisesRegex(ValueError, "Forbidden dependency"):
                                guard.check(candidate)

    def test_cycle_is_rejected_even_if_individual_edges_were_allowed(self):
        metadata = {"workspace_members": list(guard.ALLOWED), "packages": [
            {"id": name, "name": name, "manifest_path": f"/repo/crates/{name}/Cargo.toml",
             "dependencies": []} for name in guard.ALLOWED]}
        for owner in ("gateway-domain", "gateway-application"):
            dependency = "gateway-domain" if owner == "gateway-application" else "gateway-application"
            next(p for p in metadata["packages"] if p["name"] == owner)["dependencies"] = [
                {"name": dependency, "path": f"/repo/crates/{dependency}"}]
        saved = guard.ALLOWED["gateway-domain"]
        try:
            guard.ALLOWED["gateway-domain"] = saved | {"gateway-application"}
            with self.assertRaisesRegex(ValueError, "Dependency cycle"):
                guard.check(metadata)
        finally:
            guard.ALLOWED["gateway-domain"] = saved
