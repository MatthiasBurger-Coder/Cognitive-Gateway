"""Adapters remain optional outer components and keep live qualification opt-in."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ModelAdaptersArchitecture(unittest.TestCase):
    def test_adapter_dependencies_point_inward(self):
        for layer in ('gateway-domain', 'gateway-application'):
            manifest = (ROOT / 'crates' / layer / 'Cargo.toml').read_text()
            for dependency in ('gateway-daemon', 'reqwest', 'async-openai'):
                self.assertNotIn(dependency, manifest)
        source = (ROOT / 'crates/gateway-daemon/src/model_adapters.rs').read_text()
        for boundary in ('BoundedInvocationPort', 'r.context.check()', 'check_context', 'env_clear()', 'Stdio::null()'):
            self.assertIn(boundary, source)
        docker = (ROOT / 'services/local-model/Dockerfile').read_text()
        self.assertIn('services/local-model/canonical.py', docker)

    def test_live_profile_contains_only_runtime_credential_reference(self):
        fixture = json.loads((ROOT / 'tests/fixtures/model-adapters-v1/live-profile.example.json').read_text())
        self.assertEqual(fixture['adapters'][1]['credential_env'], 'CG_INDEPENDENT_PROVIDER_KEY')
        source = (ROOT / 'crates/gateway-daemon/tests/model_adapters.rs').read_text()
        self.assertIn('#[ignore = "requires CG_MODEL_ADAPTER_LIVE_PROFILE', source)
        self.assertIn('semantic_qualification', source)

    def test_coverage_gate_is_required_and_rejects_invalid_reports(self):
        gate = next(g for g in json.loads((ROOT / 'scripts/quality-gates.json').read_text())
                    if g['name'].startswith('EPIC-06.09'))
        for requirement in ('--test model_adapters', '--test invocation_host', '--locked', '--fail-under=95', '--self-test'):
            self.assertIn(requirement, gate['command'])
        spec = importlib.util.spec_from_file_location('adapter_coverage', ROOT / 'scripts/check-model-adapters-coverage.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.self_test()

    def test_general_coverage_keeps_live_profile_opt_in(self):
        gates = json.loads((ROOT / 'scripts/quality-gates.json').read_text())
        integration_gates = [g for g in gates if '--include-ignored' in g['command']]
        self.assertEqual({g['name'] for g in integration_gates}, {
            'CG-03 registry CLI coverage gate',
            'CG-16 retrieval pipeline coverage gate',
        })
        for gate in integration_gates:
            # PostgreSQL tests remain included; external live endpoints require
            # the separate explicit profile rather than a general CI runner.
            self.assertIn('--include-ignored --skip live_adapter_profile', gate['command'])
        registry_gate = next(g for g in integration_gates
                             if g['name'] == 'CG-03 registry CLI coverage gate')
        self.assertIn('--fail-under-lines 95', registry_gate['command'])
