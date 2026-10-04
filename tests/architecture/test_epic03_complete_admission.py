"""Admission must reject incomplete, inconsistent or below-threshold extended proofs."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

cg30 = module('complete_cg30', 'scripts/qualify-cg30.py')
coverage = module('complete_coverage', 'scripts/check-epic03-coverage.py')


class CompleteAdmissionTests(unittest.TestCase):
    def fixture(self, root):
        subprocess.run([sys.executable, ROOT / 'scripts/benchmark-offline-learning.py', root / 'epic03-ml-experiment.json'], check=True, capture_output=True)
        manifests = []
        for version in (2, 3):
            model_path = root / f'epic03-cpu-model-v{version}.json'
            evaluation_path = root / f'epic03-cpu-evaluation-v{version}.json'
            model_path.write_text(json.dumps({'version': version, 'artifact': {'prior_artifact_digest': manifests[0]['training']['candidate']['artifact_digest'] if manifests else None}}))
            evaluation_path.write_text(json.dumps({'status': 'PASS', 'version': version, 'prior': {'f1': 1} if version == 3 else None, 'checks': {'prior': True}}))
            manifests.append({'training': {'candidate': {'version': version, 'artifact_digest': cg30.sha256(model_path)}},
                              'evaluation': {'evidence': 'sha256-' + cg30.sha256(evaluation_path)}})
        files = {'epic03-live-release.json': {'status': 'PASS', 'real_cpu_training': True, 'postgres_restart': True,
                 'inference_versions': [2, 3, 2], 'revoked_qualification_refused': True, 'prior_comparison': True,
                 'test_used_for_selection': False, 'metrics': {'cases': 24, 'tp': 12, 'tn': 12, 'fp': 0, 'fn': 0},
                 'baseline': {}, 'journal': {'manifests': manifests, 'events': [{}] * 8 + [{'action': 'ROLLBACK'}]}},
                 'epic03-durable-workers.json': {'status': 'PASS', 'coordinator_restart': True, 'late_results_fenced': True,
                 'competing_coordinators_single_claim': True, 'duplicate_commits': 0},
                 'epic03-worker-container.json': {'status': 'PASS', 'authority_credentials': False, 'production_mounts': False,
                 'worker_uid': '10001:10001', 'image_digest': 'sha256:fixture', 'limits': {'Memory': 268435456,
                 'NanoCpus': 1000000000, 'PidsLimit': 32, 'NetworkMode': 'none', 'ReadonlyRootfs': True, 'CapDrop': ['ALL']}},
                 'epic03-python-coverage.json': {'totals': {'num_statements': 100, 'percent_covered': 95}}}
        files['epic03-ml-experiment.json'] = json.loads((root / 'epic03-ml-experiment.json').read_text())
        for name, value in files.items():
            (root / name).write_text(json.dumps(value))
        return files

    def test_complete_evidence_and_all_critical_refusals(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            files = self.fixture(root)
            self.assertEqual(cg30.validate_extended(root)['worker_consistency'], 'PASS')
            changes = [
                ('epic03-ml-experiment.json', lambda r: r.update(status='FAIL')),
                ('epic03-ml-experiment.json', lambda r: r.update(reproduced=False)),
                ('epic03-ml-experiment.json', lambda r: r['model'].update(artifact_digest='wrong')),
                ('epic03-ml-experiment.json', lambda r: r['evaluation'].update(test_used_for_selection=True)),
                ('epic03-ml-experiment.json', lambda r: r['evaluation']['checks'].update(baseline=False)),
                ('epic03-ml-experiment.json', lambda r: r.update(random_trials=0)),
                ('epic03-ml-experiment.json', lambda r: r['drift'].update(authority_changed=True)),
                ('epic03-ml-experiment.json', lambda r: r['performance'].update(peak_process_rss_bytes=0)),
                ('epic03-live-release.json', lambda r: r.update(postgres_restart=False)),
                ('epic03-live-release.json', lambda r: r.update(inference_versions=[2, 3, 3])),
                ('epic03-live-release.json', lambda r: r.update(prior_comparison=False)),
                ('epic03-live-release.json', lambda r: r['metrics'].update(fp=1)),
                ('epic03-live-release.json', lambda r: r['journal']['events'].pop()),
                ('epic03-durable-workers.json', lambda r: r.update(duplicate_commits=1)),
                ('epic03-worker-container.json', lambda r: r.update(authority_credentials=True)),
                ('epic03-worker-container.json', lambda r: r['limits'].update(NetworkMode='bridge')),
                ('epic03-python-coverage.json', lambda r: r['totals'].update(percent_covered=94.9)),
            ]
            for name, mutate in changes:
                modified = copy.deepcopy(files[name])
                mutate(modified)
                (root / name).write_text(json.dumps(modified))
                with self.subTest(name=name), self.assertRaises(ValueError):
                    cg30.validate_extended(root)
                (root / name).write_text(json.dumps(files[name]))
            (root / 'epic03-cpu-model-v2.json').write_text('changed')
            with self.assertRaises(ValueError):
                cg30.validate_extended(root)
            (root / 'epic03-live-release.json').unlink()
            with self.assertRaises(OSError):
                cg30.validate_extended(root)

    def test_each_new_module_requires_real_coverage(self):
        good = {'data': [{'files': [{'filename': '/repo/' + path, 'summary': {'lines': {'count': 100, 'covered': 95}}}
                                    for path in coverage.EXPECTED]}]}
        self.assertEqual(len(coverage.check(good)), len(coverage.EXPECTED))
        for value in (94, True, 95.0, -1, 101):
            bad = copy.deepcopy(good)
            bad['data'][0]['files'][0]['summary']['lines']['covered'] = value
            with self.assertRaises(ValueError):
                coverage.check(bad)
        good['data'][0]['files'].pop()
        with self.assertRaises(ValueError):
            coverage.check(good)
