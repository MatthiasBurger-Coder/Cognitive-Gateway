"""CPU-only, proposal-producing binary classifier and reproducible offline experiments.

Only the trusted CG-28 host admits source rows. This process has no governance,
production credentials, network client, or release API. Train and evaluation are
separate invocations: final test rows cannot reach search or feature fitting.
"""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import platform
import random
import statistics
import sys
import time

VERSION = 'cg-offline-cpu-v1'


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_rows(rows, scope, allowed):
    require(0 < len(rows) <= 4096, 'bounded nonempty dataset required')
    ids, groups, sources, examples = set(), {}, {}, {}
    families, targets = {}, {}
    for row in rows:
        require(row['scope'] == scope and row['split'] in allowed, 'scope/split mismatch')
        require(row['id'] not in ids, 'duplicate identity')
        ids.add(row['id'])
        require(type(row['label']) is int and row['label'] in (0, 1), 'binary validated target required')
        require(type(row['time']) is int and row['label_basis'] and row['evidence'], 'missing target provenance')
        require(row['features'] and all(isinstance(x, (int, float)) and not isinstance(x, bool)
                and math.isfinite(x) and abs(x) <= 1e9 for x in row['features'].values()), 'invalid features')
        for field, index in [('group', groups), ('source', sources), ('example', examples)]:
            require(row[field], 'missing leakage identity')
            require(index.setdefault(row[field], row['split']) == row['split'], 'split leakage')
            if field != 'group':
                require(families.setdefault((field, row[field]), row['group']) == row['group'], 'related sources need one leakage group')
        require(targets.setdefault(row['example'], row['label']) == row['label'], 'contradictory duplicate target')
    return sorted(rows, key=lambda r: r['id'])


def fit_features(rows, names, top_k):
    require(0 < top_k <= len(names) <= 64 and len(set(names)) == len(names), 'invalid feature selection')
    require(all(set(names) <= r['features'].keys() for r in rows), 'missing feature')
    # Fit exclusively on the current training fold. Deterministic variance ranking.
    ranked = sorted(names, key=lambda n: (-statistics.pvariance([r['features'][n] for r in rows]), n))
    chosen = sorted(ranked[:top_k])
    columns = {n: {'mean': statistics.mean(r['features'][n] for r in rows),
                   'scale': statistics.pstdev(r['features'][n] for r in rows) or 1.0,
                   'min': min(r['features'][n] for r in rows),
                   'max': max(r['features'][n] for r in rows)} for n in chosen}
    return {'version': VERSION, 'selection': 'train-variance', 'columns': columns}


def vector(row, features):
    require(features['columns'].keys() <= row['features'].keys(), 'missing feature')
    return [(row['features'][n] - c['mean']) / c['scale'] for n, c in features['columns'].items()]


def fit(rows, names, top_k):
    features = fit_features(rows, names, top_k)
    classes = [[vector(r, features) for r in rows if r['label'] == y] for y in (0, 1)]
    require(all(classes), 'training must contain both targets')
    centers = [[statistics.mean(col) for col in zip(*group)] for group in classes]
    return {'family': 'nearest-centroid', 'features': features, 'centers': centers, 'temperature': 1.0}


def probability(model, row):
    if model['family'] == 'majority':
        return model['probability']
    x = vector(row, model['features'])
    distances = [sum((a - b) ** 2 for a, b in zip(x, center)) for center in model['centers']]
    z = max(-40, min(40, (distances[0] - distances[1]) / model['temperature']))
    return 1 / (1 + math.exp(-z))


def classification(labels, probabilities, threshold=0.5):
    require(len(labels) == len(probabilities) > 0 and 0 < threshold < 1, 'invalid metric input')
    require(all(type(y) is int and y in (0, 1) for y in labels)
            and all(math.isfinite(p) and 0 <= p <= 1 for p in probabilities), 'invalid metric values')
    tp = sum(y == 1 and p >= threshold for y, p in zip(labels, probabilities))
    tn = sum(y == 0 and p < threshold for y, p in zip(labels, probabilities))
    fp = sum(y == 0 and p >= threshold for y, p in zip(labels, probabilities))
    fn = sum(y == 1 and p < threshold for y, p in zip(labels, probabilities))
    precision, recall = tp / (tp + fp) if tp + fp else 0, tp / (tp + fn) if tp + fn else 0
    denominator = (tp + fp) * (tp + fn) * (tn + fp) * (tn + fn)
    bins = []
    for i in range(10):
        pairs = [(y, p) for y, p in zip(labels, probabilities) if i / 10 <= p < (i + 1) / 10 or i == 9 and p == 1]
        if pairs:
            bins.append(len(pairs) * abs(statistics.mean(p for _, p in pairs) - statistics.mean(y for y, _ in pairs)))
    return {'task': 'binary-classification', 'cases': len(labels), 'tp': tp, 'tn': tn, 'fp': fp, 'fn': fn,
            'precision': precision, 'recall': recall, 'f1': 2 * precision * recall / (precision + recall) if precision + recall else 0,
            'specificity': tn / (tn + fp) if tn + fp else None,
            'false_positive_rate': fp / (tn + fp) if tn + fp else None,
            'mcc': (tp * tn - fp * fn) / math.sqrt(denominator) if denominator else None,
            'brier': statistics.mean((p - y) ** 2 for y, p in zip(labels, probabilities)),
            'ece': sum(bins) / len(labels)}


def regression(expected, actual):
    require(len(expected) == len(actual) > 0 and all(math.isfinite(x) for x in expected + actual), 'invalid regression input')
    errors = [a - y for y, a in zip(expected, actual)]
    mse = statistics.mean(e * e for e in errors)
    variance = sum((y - statistics.mean(expected)) ** 2 for y in expected)
    return {'task': 'regression', 'cases': len(errors), 'mae': statistics.mean(abs(e) for e in errors),
            'mse': mse, 'rmse': math.sqrt(mse), 'r2': 1 - sum(e * e for e in errors) / variance if variance else None}


def ranking(relevant, returned, k):
    require(k > 0 and relevant and len(set(returned)) == len(returned), 'invalid ranking input')
    ranked = returned[:k]
    hits = [int(item in relevant) for item in ranked]
    dcg = sum(h / math.log2(i + 2) for i, h in enumerate(hits))
    ideal = sum(1 / math.log2(i + 2) for i in range(min(k, len(relevant))))
    return {'task': 'ranking', 'precision_at_k': sum(hits) / k, 'recall_at_k': sum(hits) / len(relevant),
            'mrr': next((1 / (i + 1) for i, h in enumerate(hits) if h), 0), 'ndcg': dcg / ideal}


def folds(rows, strategy, count):
    require(strategy in ('group', 'chronological') and type(count) is int and 2 <= count <= 10, 'explicit CV strategy required')
    groups = sorted({r['group'] for r in rows}, key=lambda g: (min(r['time'] for r in rows if r['group'] == g), g))
    require(len(groups) >= count + 1, 'insufficient independent groups')
    block_count = count + 1 if strategy == 'chronological' else count
    blocks = [groups[i * len(groups) // block_count:(i + 1) * len(groups) // block_count] for i in range(block_count)]
    for index in range(1 if strategy == 'chronological' else 0, block_count):
        val_groups = set(blocks[index])
        train_groups = set(itertools.chain.from_iterable(blocks[:index] if strategy == 'chronological'
                                                        else blocks[:index] + blocks[index + 1:]))
        train = [r for r in rows if r['group'] in train_groups]
        val = [r for r in rows if r['group'] in val_groups]
        if strategy == 'chronological':
            require(max(r['time'] for r in train) < min(r['time'] for r in val), 'chronological group overlap')
        yield train, val


def metrics(model, rows):
    return classification([r['label'] for r in rows], [probability(model, r) for r in rows])


def train(request):
    rows = validate_rows(request['rows'], request['scope'], {'TRAIN', 'VALIDATION'})
    train_rows = [r for r in rows if r['split'] == 'TRAIN']
    validation = [r for r in rows if r['split'] == 'VALIDATION']
    plan = request['plan']
    require(set(plan) == {'feature_schema_version', 'features', 'top_k', 'temperatures', 'cv', 'folds', 'search', 'seed', 'budget', 'objective', 'stop_f1'}, 'unsupported experiment option')
    require(train_rows and validation and plan['feature_schema_version'] and plan['search'] in ('grid', 'random'), 'invalid experiment plan')
    require(plan['objective'] == 'f1' and type(plan['budget']) is int and 0 < plan['budget'] <= 128, 'bounded objective/search required')
    require(plan['stop_f1'] is None or isinstance(plan['stop_f1'], (int, float)) and not isinstance(plan['stop_f1'], bool) and math.isfinite(plan['stop_f1']) and 0 <= plan['stop_f1'] <= 1, 'invalid stop condition')
    require(type(plan['seed']) is int and 0 <= plan['seed'] <= 2**64 - 1, 'invalid random seed')
    require(all(isinstance(n, str) and n.strip() for n in plan['features']), 'invalid feature names')
    require(plan['top_k'] and all(type(k) is int for k in plan['top_k']) and len(plan['top_k']) <= 64 and plan['temperatures'] and len(plan['temperatures']) <= 32
            and all(isinstance(t, (int, float)) and not isinstance(t, bool) and math.isfinite(t) and t > 0 for t in plan['temperatures']), 'invalid search space')
    require(all(set(r['features']) == set(plan['features']) for r in rows), 'feature schema mismatch')
    require(set(plan['features']).isdisjoint({'label', 'success', 'failure', 'outcome'}), 'target leakage in feature schema')
    if plan['cv'] == 'chronological':
        require(max(r['time'] for r in train_rows) < min(r['time'] for r in validation), 'future validation leakage')
    splits = list(folds(train_rows, plan['cv'], plan['folds']))
    search = list(itertools.product(sorted(set(plan['top_k'])), sorted(set(plan['temperatures']))))
    if plan['search'] == 'random':
        random.Random(plan['seed']).shuffle(search)
    baseline = {'family': 'majority', 'probability': sum(r['label'] for r in train_rows) / len(train_rows)}
    trials = []
    for top_k, temperature in search[:plan['budget']]:
        scores = []
        lineage = []
        for training, held in splits:
            model = fit(training, plan['features'], top_k)
            model['temperature'] = temperature
            scores.append(metrics(model, held)['f1'])
            lineage.append({'fit': [r['id'] for r in training], 'score': [r['id'] for r in held],
                            'feature_digest': digest(model['features'])})
        trials.append({'top_k': top_k, 'temperature': temperature, 'cv_f1': statistics.mean(scores), 'folds': lineage})
        if plan['stop_f1'] is not None and trials[-1]['cv_f1'] >= plan['stop_f1']:
            break
    chosen = sorted(trials, key=lambda t: (-t['cv_f1'], t['top_k'], t['temperature']))[0]
    candidate = fit(train_rows, plan['features'], chosen['top_k'])
    candidate['temperature'] = chosen['temperature']
    before = metrics(candidate, validation)
    # Temperature is calibrated solely on validation. CV fits never see validation.
    temperature_scores = []
    for temperature in sorted(set(plan['temperatures'])):
        calibrated = {**candidate, 'temperature': temperature}
        temperature_scores.append((metrics(calibrated, validation)['brier'], temperature))
    candidate['temperature'] = min(temperature_scores)[1]
    after = metrics(candidate, validation)
    prior = request.get('prior')
    if prior is not None:
        require(prior['scope'] == request['scope'] and prior['feature_schema_version'] == plan['feature_schema_version'], 'prior model scope/schema mismatch')
    result = {'version': VERSION, 'scope': request['scope'], 'job': request['job'],
              'dataset_digest': request['dataset_digest'], 'recipe_digest': request['recipe_digest'],
              'feature_schema_version': plan['feature_schema_version'], 'feature_schema_digest': digest(plan['features']),
              'model': candidate, 'baseline': baseline, 'prior': prior['model'] if prior else None,
              'plan': plan, 'trials': trials, 'chosen': chosen,
              'calibration': {'fit_ids': [r['id'] for r in validation], 'before': before, 'after': after},
              'validation_baseline': metrics(baseline, validation),
              'validation_prior': metrics(prior['model'], validation) if prior else None,
              'source_digest': digest(rows), 'source_bindings': [{k: r[k] for k in ('id', 'group', 'source', 'example')} for r in rows],
              'training_range': [min(r['time'] for r in rows), max(r['time'] for r in rows)],
              'runtime': {'python': platform.python_version(), 'implementation': platform.python_implementation(),
                          'code_digest': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},
              'prior_artifact_digest': request.get('prior_artifact_digest'), 'test_used_for_selection': False}
    return {'artifact': result, 'artifact_digest': digest(result)}


def predict(artifact, row):
    columns = artifact['model']['features']['columns']
    require(columns.keys() <= row['features'].keys(), 'missing feature')
    x = vector(row, artifact['model']['features'])
    if any(not math.isfinite(v) for v in x):
        raise ValueError('nonfinite input')
    if any(row['features'][n] < c['min'] - 3 * c['scale'] or row['features'][n] > c['max'] + 3 * c['scale'] for n, c in columns.items()):
        return {'label': None, 'confidence': None, 'disposition': 'COGNITIVE_ESCALATION_REQUIRED', 'runtime': {'python': platform.python_version()}}
    p = probability(artifact['model'], row)
    return {'label': int(p >= 0.5), 'confidence': max(p, 1 - p), 'probability': p, 'disposition': 'PROPOSAL', 'runtime': {'python': platform.python_version()}}


def evaluate(request):
    artifact = request['artifact']
    require(digest(artifact) == request['artifact_digest'], 'changed model artifact')
    rows = validate_rows(request['rows'], artifact['scope'], {'TEST'})
    fitted = artifact['source_bindings']
    require(all({r[field] for r in rows}.isdisjoint(s[field] for s in fitted)
                for field in ('id', 'group', 'source', 'example')), 'test overlaps fitted data')
    if artifact['plan']['cv'] == 'chronological':
        require(min(r['time'] for r in rows) > artifact['training_range'][1], 'future knowledge in training')
    expected = [r['label'] for r in rows]
    predictions = [predict(artifact, r) for r in rows]
    scored = metrics(artifact['model'], rows)
    baseline = metrics(artifact['baseline'], rows)
    prior = metrics(artifact['prior'], rows) if artifact['prior'] else None
    profile = request['profile']
    require(profile['task'] == 'binary-classification' and profile['version'] > 0, 'task-specific profile required')
    require(0 <= profile['max_regression'] <= 1 and profile['floors'] and profile['ceilings'], 'invalid evaluation profile')
    supported = {'precision', 'recall', 'f1', 'specificity', 'false_positive_rate', 'brier', 'ece', 'mcc'}
    require(set(profile['floors']) | set(profile['ceilings']) <= supported, 'unsupported profile metric')
    require(all(isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) and (-1 if k == 'mcc' else 0) <= v <= 1
                for k, v in list(profile['floors'].items()) + list(profile['ceilings'].items())), 'invalid metric threshold')
    checks = {name: scored[name] is not None and scored[name] >= floor for name, floor in profile['floors'].items()}
    checks.update({name: scored[name] is not None and scored[name] <= ceiling for name, ceiling in profile['ceilings'].items()})
    checks['baseline'] = scored['f1'] + profile['max_regression'] >= baseline['f1']
    checks['prior'] = prior is None or scored['f1'] + profile['max_regression'] >= prior['f1']
    checks['no_abstention'] = all(p['label'] is not None for p in predictions)
    return {'status': 'PASS' if all(checks.values()) else 'FAIL', 'checks': checks,
            'profile_digest': digest(profile), 'metrics': scored, 'baseline': baseline, 'prior': prior,
            'predictions': [{'id': r['id'], 'expected': y, **p} for r, y, p in zip(rows, expected, predictions)],
            'artifact_digest': request['artifact_digest'], 'test_digest': digest(rows), 'test_used_for_selection': False}


def drift(artifact, rows, observed_labels, profile):
    require(rows and len(rows) == len(observed_labels), 'missing drift outcomes')
    probabilities = [probability(artifact['model'], r) for r in rows]
    result = classification(observed_labels, probabilities)
    ood = sum(predict(artifact, r)['label'] is None for r in rows) / len(rows)
    shifted = result['f1'] < profile['min_f1'] or result['brier'] > profile['max_brier'] or ood > profile['max_ood_rate']
    return {'action': 'REEVALUATE_RETRAIN_OR_ROLLBACK' if shifted else 'RETAIN',
            'metrics': result, 'ood_rate': ood, 'authority_changed': False, 'profile_digest': digest(profile)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['train', 'evaluate', 'predict', 'execute'])
    args = parser.parse_args()
    request = json.load(sys.stdin)
    if 'prior_json' in request:
        original = request.pop('prior_json')
        require(hashlib.sha256(original.encode()).hexdigest() == request['prior_artifact_digest'], 'changed prior artifact')
        prior = json.loads(original)
        require(digest(prior['artifact']) == prior['artifact_digest'], 'invalid prior envelope')
        request['prior'] = prior['artifact']
    if 'artifact_json' in request:
        envelope = json.loads(request.pop('artifact_json'))
        require(set(envelope) == {'artifact', 'artifact_digest'}, 'invalid artifact envelope')
        request.update(envelope)
    if args.operation == 'execute':
        require(request['kind'] in ('MODEL', 'EVALUATION'), 'unsupported worker kind')
        args.operation = 'predict' if request['kind'] == 'MODEL' else 'evaluate'
    result = train(request) if args.operation == 'train' else evaluate(request) if args.operation == 'evaluate' else predict(request['artifact'], request['row'])
    print(json.dumps(result, allow_nan=False))


if __name__ == '__main__':
    try:
        main()
    except (KeyError, ValueError, TypeError, ZeroDivisionError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
