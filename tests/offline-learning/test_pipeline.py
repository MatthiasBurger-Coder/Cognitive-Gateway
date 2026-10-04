import copy
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import io
import hashlib
import runpy
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('pipeline', ROOT / 'services/offline-learning/pipeline.py')
pipeline = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pipeline)


def rows(split, start, count=24):
    return [{'id': f'signal-{n}', 'scope': 'project-a', 'split': split, 'time': n,
             'label': n % 2, 'label_basis': 'verified-outcome-v1', 'evidence': [f'evidence-{n}'],
             'group': f'episode-{n // 2}', 'source': f'snapshot-{n}', 'example': f'example-{n}',
             'features': {'health': (-1 if n % 2 == 0 else 1) * (1 + n / 1000), 'noise': n % 3 / 1000}}
            for n in range(start, start + count)]


def request():
    return {'scope': 'project-a', 'job': 'job', 'dataset_digest': 'a' * 64, 'recipe_digest': 'b' * 64,
            'rows': rows('TRAIN', 0, 48) + rows('VALIDATION', 48),
            'plan': {'feature_schema_version': 'health-v1', 'features': ['health', 'noise'],
                     'top_k': [1, 2], 'temperatures': [0.5, 1, 2], 'cv': 'chronological', 'folds': 3,
                     'search': 'grid', 'seed': 42, 'budget': 6, 'objective': 'f1', 'stop_f1': None}}


def profile():
    return {'task': 'binary-classification', 'version': 1, 'floors': {'precision': 1, 'recall': 1, 'f1': 1},
            'ceilings': {'false_positive_rate': 0, 'brier': 0.01, 'ece': 0.1}, 'max_regression': 0}


class PipelineTests(unittest.TestCase):
    def test_real_training_reproducibility_and_held_out_evaluation(self):
        req = request()
        trained = pipeline.train(req)
        req['rows'].reverse()
        self.assertEqual(trained, pipeline.train(req))
        artifact = trained['artifact']
        self.assertFalse(artifact['test_used_for_selection'])
        self.assertEqual(artifact['model']['features']['selection'], 'train-variance')
        self.assertLessEqual(artifact['calibration']['after']['brier'], artifact['calibration']['before']['brier'])
        test = rows('TEST', 72)
        result = pipeline.evaluate({**trained, 'rows': test, 'profile': profile()})
        self.assertEqual(result['status'], 'PASS')
        self.assertEqual(result['metrics']['fp'], 0)
        self.assertGreater(result['metrics']['f1'], result['baseline']['f1'])
        req['prior'] = artifact
        trained = pipeline.train(req)
        self.assertIsNotNone(pipeline.evaluate({**trained, 'rows': test, 'profile': profile()})['prior'])
        for trial in artifact['trials']:
            for fold in trial['folds']:
                self.assertTrue(set(fold['fit']).isdisjoint(fold['score']))
        # Test labels/features cannot change a selected artifact: they enter only evaluation.
        altered = copy.deepcopy(test)
        for row in altered:
            row['label'] = 1 - row['label']
        self.assertEqual(pipeline.evaluate({**trained, 'rows': altered, 'profile': profile()})['status'], 'FAIL')

    def test_group_cv_random_search_and_early_stop_are_bounded(self):
        req = request()
        req['plan'].update(cv='group', search='random', budget=2, stop_f1=1)
        first = pipeline.train(req)
        self.assertEqual(first, pipeline.train(req))
        self.assertEqual(len(first['artifact']['trials']), 1)
        self.assertTrue(all(set(f['fit']).isdisjoint(f['score']) for t in first['artifact']['trials'] for f in t['folds']))

    def test_source_and_search_rejections(self):
        for mutate in [
            lambda r: r['rows'][0].update(split='TEST'),
            lambda r: r['rows'][0].update(scope='other'),
            lambda r: r['rows'][0].update(label=True),
            lambda r: r['rows'][0].update(label_basis=''),
            lambda r: r['rows'][0]['features'].update(health=math.nan),
            lambda r: r['rows'][0]['features'].update(health=True),
            lambda r: r['rows'][48].update(group=r['rows'][0]['group']),
            lambda r: r['rows'][48].update(source=r['rows'][0]['source']),
            lambda r: r['rows'][48].update(example=r['rows'][0]['example']),
            lambda r: r['rows'].append(r['rows'][0]),
            lambda r: r['plan'].update(cv='random'),
            lambda r: r['plan'].update(folds=100),
            lambda r: r['plan'].update(budget=0),
            lambda r: r['plan'].update(objective='test_f1'),
            lambda r: r['plan'].update(top_k=[0]),
            lambda r: r['plan'].update(temperatures=[0]),
            lambda r: r['plan'].update(stop_f1=2),
            lambda r: r['plan'].update(stop_f1=True),
            lambda r: r['plan'].update(temperatures=[True]),
            lambda r: r['plan'].update(folds=True),
            lambda r: r['plan'].update(seed=True),
            lambda r: r['plan'].update(unknown_option=True),
            lambda r: r['rows'][48].update(time=0),
            lambda r: r['rows'][0]['features'].pop('health'),
            lambda r: r['plan'].update(features=['success']),
            lambda r: r.update(prior={'scope': 'other', 'feature_schema_version': 'health-v1'}),
        ]:
            req = request()
            mutate(req)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                pipeline.train(req)
        with self.assertRaises(ValueError):
            pipeline.fit_features(rows('TRAIN', 0), ['health', 'health'], 1)
        with self.assertRaises(ValueError):
            pipeline.fit_features(rows('TRAIN', 0), ['missing'], 1)
        with self.assertRaises(ValueError):
            pipeline.fit([r for r in rows('TRAIN', 0) if r['label'] == 0], ['health'], 1)
        with self.assertRaises(ValueError):
            list(pipeline.folds(rows('TRAIN', 0, 2), 'group', 3))
        overlapping = rows('TRAIN', 0)
        overlapping[-1]['time'] = -1
        with self.assertRaises(ValueError):
            list(pipeline.folds(overlapping, 'chronological', 3))

    def test_evaluation_tampering_leakage_and_profile_rejections(self):
        trained = pipeline.train(request())
        for mutate in [
            lambda r: r['artifact'].update(scope='other'),
            lambda r: r['rows'][0].update(split='TRAIN'),
            lambda r: r['rows'][0].update(id='signal-0'),
            lambda r: r['rows'][0].update(group='episode-0'),
            lambda r: r['rows'][0].update(source='snapshot-0'),
            lambda r: r['rows'][0].update(example='example-0'),
            lambda r: r['rows'][0].update(time=0),
            lambda r: r['profile'].update(task='regression'),
            lambda r: r['profile'].update(floors={}),
            lambda r: r['profile']['floors'].update(unknown=1),
        ]:
            req = {**copy.deepcopy(trained), 'rows': rows('TEST', 72), 'profile': profile()}
            mutate(req)
            with self.assertRaises(ValueError):
                pipeline.evaluate(req)

    def test_ood_missing_features_and_drift_require_escalation(self):
        artifact = pipeline.train(request())['artifact']
        test = rows('TEST', 72)
        self.assertEqual(pipeline.predict(artifact, test[0])['disposition'], 'PROPOSAL')
        shifted = copy.deepcopy(test)
        for r in shifted:
            r['features']['health'] *= 100
        self.assertIsNone(pipeline.predict(artifact, shifted[0])['label'])
        drift_profile = {'min_f1': 0.9, 'max_brier': 0.1, 'max_ood_rate': 0}
        self.assertEqual(pipeline.drift(artifact, test, [r['label'] for r in test], drift_profile)['action'], 'RETAIN')
        result = pipeline.drift(artifact, shifted, [1 - r['label'] for r in shifted], drift_profile)
        self.assertEqual(result['action'], 'REEVALUATE_RETRAIN_OR_ROLLBACK')
        self.assertFalse(result['authority_changed'])
        with self.assertRaises(ValueError):
            pipeline.predict(artifact, {'features': {}})
        with self.assertRaises(ValueError):
            pipeline.predict(artifact, {'features': {'health': math.inf}})
        trained = {'artifact': artifact, 'artifact_digest': pipeline.digest(artifact)}
        self.assertEqual(pipeline.evaluate({**trained, 'rows': shifted, 'profile': profile()})['status'], 'FAIL')

    def test_task_specific_metrics_and_degenerate_denominators(self):
        binary = pipeline.classification([0, 1], [0, 1])
        self.assertEqual(binary['mcc'], 1)
        empty_class = pipeline.classification([1, 1], [0, 0])
        self.assertIsNone(empty_class['specificity'])
        self.assertEqual(empty_class['f1'], 0)
        self.assertEqual(pipeline.regression([1, 2], [1, 2])['r2'], 1)
        self.assertIsNone(pipeline.regression([1, 1], [1, 2])['r2'])
        self.assertEqual(pipeline.ranking({'a'}, ['a', 'b'], 2)['mrr'], 1)
        self.assertEqual(pipeline.ranking({'a'}, ['b'], 1)['mrr'], 0)
        for operation in [lambda: pipeline.classification([], []), lambda: pipeline.classification([0], [2]),
                          lambda: pipeline.regression([1], [math.inf]), lambda: pipeline.ranking({'a'}, ['a', 'a'], 2),
                          lambda: pipeline.drift({}, [], [], {})]:
            with self.assertRaises(ValueError):
                operation()

    def test_in_process_cli_covers_the_artifact_transport_and_failures(self):
        trained = pipeline.train(request())
        prior_json = json.dumps(trained) + '\n'
        with_prior = {**request(), 'prior_json': prior_json, 'prior_artifact_digest': hashlib.sha256(prior_json.encode()).hexdigest()}
        for operation, value in [('train', request()), ('train', with_prior), ('evaluate', {**trained, 'artifact_json': json.dumps(trained), 'rows': rows('TEST', 72), 'profile': profile()}), ('predict', {'artifact_json': json.dumps(trained), 'row': rows('TEST', 72)[0]}), ('execute', {'kind': 'MODEL', 'artifact_json': json.dumps(trained), 'row': rows('TEST', 72)[0]}), ('execute', {'kind': 'EVALUATION', 'artifact_json': json.dumps(trained), 'rows': rows('TEST', 72), 'profile': profile()})]:
            with patch.object(sys, 'argv', ['pipeline.py', operation]), patch.object(sys, 'stdin', io.StringIO(json.dumps(value))), patch.object(sys, 'stdout', io.StringIO()) as output:
                runpy.run_path(str(ROOT / 'services/offline-learning/pipeline.py'), run_name='__main__')
                self.assertTrue(json.loads(output.getvalue()))
        with patch.object(sys, 'argv', ['pipeline.py', 'train']), patch.object(sys, 'stdin', io.StringIO('{}')), patch.object(sys, 'stderr', io.StringIO()), self.assertRaises(SystemExit) as failure:
            runpy.run_path(str(ROOT / 'services/offline-learning/pipeline.py'), run_name='__main__')
        self.assertEqual(failure.exception.code, 1)

    def test_cli_executes_real_training_and_refuses_bad_input(self):
        script = ROOT / 'services/offline-learning/pipeline.py'
        trained = subprocess.run([sys.executable, script, 'train'], input=json.dumps(request()), text=True, capture_output=True, check=True)
        artifact = json.loads(trained.stdout)
        evaluated = subprocess.run([sys.executable, script, 'evaluate'], input=json.dumps({**artifact, 'rows': rows('TEST', 72), 'profile': profile()}), text=True, capture_output=True, check=True)
        self.assertEqual(json.loads(evaluated.stdout)['status'], 'PASS')
        prediction = subprocess.run([sys.executable, script, 'predict'], input=json.dumps({'artifact': artifact['artifact'], 'row': rows('TEST', 72)[0]}), text=True, capture_output=True, check=True)
        self.assertEqual(json.loads(prediction.stdout)['proposal'] if 'proposal' in json.loads(prediction.stdout) else json.loads(prediction.stdout)['label'], 0)
        self.assertNotEqual(subprocess.run([sys.executable, script, 'train'], input='{}', text=True, capture_output=True).returncode, 0)


if __name__ == '__main__':
    unittest.main()
