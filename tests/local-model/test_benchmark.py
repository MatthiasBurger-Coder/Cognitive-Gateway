import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'services/local-model'))
import benchmark as b


class BenchmarkTests(unittest.TestCase):
    def setUp(self):
        self.dataset = b.load(ROOT / 'models/datasets/cognitive-signals-v1.json')
        self.profile = b.fixture_profile(self.dataset)

    def test_four_use_cases_reproducible_inputs_and_proposal_provenance(self):
        report = b.run(self.dataset, self.profile, b.FixtureAdapter(), rounds=2)
        self.assertEqual(report['status'], 'completed')
        self.assertFalse(report['product_claim'])
        self.assertEqual(len(report['samples']), 1 + 2 * len(self.dataset['cases']))
        self.assertEqual(set(report['summary']['quality_by_task']), b.TASKS)
        for group in report['summary']['quality_by_task'].values():
            self.assertEqual(group['exact_match_accuracy'], 1)
        replay = b.run(**dict(dataset=report['inputs']['dataset'], profile=report['inputs']['profile'],
                             adapter=b.FixtureAdapter(), rounds=report['inputs']['warm_rounds']))
        self.assertEqual([s['result']['proposal'] for s in replay['samples']],
                         [s['result']['proposal'] for s in report['samples']])
        self.assertEqual(report['dataset']['digest'], b.digest(self.dataset))
        self.assertIsNone(report['summary']['warm']['mean_generated_tokens_per_second'])
        self.assertIsNone(report['samples'][0]['memory']['resident_model_bytes'])
        for sample in report['samples']:
            self.assertEqual(sample['result']['kind'], 'proposal')
            provenance = sample['result']['provenance']
            self.assertEqual(provenance['model_version'], self.profile['model_version'])
            self.assertEqual(provenance['system_digest'], b.digest(self.dataset['tasks'][sample['task']]['system']))

    def test_labels_are_not_inference_inputs(self):
        altered = copy.deepcopy(self.dataset)
        altered['cases'][0]['expected']['label'] = 'write'
        report = b.run(altered, self.profile, b.FixtureAdapter(), rounds=1)
        self.assertEqual(report['status'], 'failed')
        self.assertEqual(report['samples'][0]['result']['proposal'], {'label': 'read'})
        self.assertLess(report['summary']['warm']['exact_match_accuracy'], 1)
        self.assertNotEqual(report['dataset']['digest'], b.digest(self.dataset))

    def test_adapter_replacement_failure_and_no_state_mutation(self):
        replacement = copy.deepcopy(self.profile)
        replacement['model_id'] = 'replacement'
        replacement['model_version'] = '2'
        report = b.run(self.dataset, replacement, b.FixtureAdapter(), 1)
        self.assertEqual(report['status'], 'completed')
        self.assertEqual(report['samples'][0]['result']['model_id'], 'replacement')
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / 'registry.json'
            state.write_text('{"Process":"START","Policy":"deny","aliases":{"role":"original"}}')
            before = state.read_bytes()
            with patch.dict(os.environ, {'CG_MODEL_REGISTRY': str(state)}):
                adapter = b.FixtureAdapter()
                with patch.object(adapter, 'generate', side_effect=b.ModelError('runtime_unavailable')):
                    report = b.run(self.dataset, self.profile, adapter, 1)
            self.assertEqual(state.read_bytes(), before)
        self.assertEqual(report['fallback'], 'deterministic-core')
        self.assertNotEqual(report['status'], 'completed')
        self.assertEqual(len(report['samples']), 1)
        self.assertEqual(report['summary']['cold']['exact_match_accuracy'], 0)

    def test_gpu_unavailable_is_separate_from_cpu(self):
        adapter = b.FixtureAdapter()
        adapter.acceleration = 'nvidia'
        with patch.object(adapter, 'memory', return_value={'resident_model_bytes': 100, 'vram_bytes': 0}):
            report = b.run(self.dataset, self.profile, adapter, 1)
        self.assertEqual(report['status'], 'unavailable')
        self.assertEqual(report['acceleration'], 'nvidia')
        self.assertEqual(report['error'], 'hardware_unavailable')
        self.assertEqual(len(report['samples']), 1)
        adapter.acceleration = 'cpu'
        with patch.object(adapter, 'memory', return_value={'resident_model_bytes': 100, 'vram_bytes': 40}):
            report = b.run(self.dataset, self.profile, adapter, 1)
        self.assertEqual(report['status'], 'failed')
        self.assertEqual(report['samples'][0]['error'], 'cpu_gate_failed')

    def test_invalid_inputs_and_provenance_fail_closed(self):
        for field, value in [('artifact_digest', None), ('template_digest', None),
                             ('capabilities', ['unsupported']), ('supported_input_contracts', ['foreign'])]:
            profile = {**self.profile, field: value}
            with self.assertRaises(b.ModelError):
                b.run(self.dataset, profile, b.FixtureAdapter(), 1)
        duplicated = copy.deepcopy(self.dataset)
        duplicated['cases'].append(duplicated['cases'][0])
        with self.assertRaisesRegex(b.ModelError, 'dataset_duplicate_case'):
            b.run(duplicated, self.profile, b.FixtureAdapter(), 1)
        for rounds in (0, 101):
            with self.assertRaisesRegex(b.ModelError, 'rounds_invalid'):
                b.run(self.dataset, self.profile, b.FixtureAdapter(), rounds)
        profile = {**self.profile, 'artifact_digest': 'sha256:' + 'a' * 64}
        report = b.run(self.dataset, profile, b.FixtureAdapter(), 1)
        self.assertEqual(report['error'], 'provenance_changed')
        self.assertEqual(report['samples'], [])

    def test_bad_output_identity_and_provenance_are_not_scored_as_success(self):
        for change in ('schema', 'identity', 'provenance', 'metrics'):
            adapter = b.FixtureAdapter()
            generate = adapter.generate
            def bad(*args, **kwargs):
                value = generate(*args, **kwargs)
                if change == 'schema':
                    value['proposal'] = {'label': 'invented'}
                elif change == 'identity':
                    value['model_id'] = 'unrequested'
                elif change == 'provenance':
                    value['provenance']['system_digest'] = 'mutable'
                else:
                    value['metrics']['latency_seconds'] = float('nan')
                return value
            with patch.object(adapter, 'generate', side_effect=bad):
                report = b.run(self.dataset, self.profile, adapter, 1)
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(report['summary']['cold']['successes'], 0)

    def test_cli_records_fixture_evidence_and_preserves_existing_report(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'report.json'
            command = [sys.executable, str(ROOT / 'scripts/benchmark-local-model.py'),
                       '--warm-rounds', '1', '--output', str(output)]
            first = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            original = output.read_bytes()
            self.assertEqual(json.loads(original)['evidence_kind'], 'deterministic-fixture')
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual(output.read_bytes(), original)
            self.assertNotEqual(subprocess.run(command + ['--acceleration', 'nvidia'],
                                              capture_output=True).returncode, 0)


if __name__ == '__main__':
    unittest.main()
