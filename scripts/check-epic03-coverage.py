#!/usr/bin/env python3
"""Require 95% measured Rust line coverage for every new EPIC-03 runtime module."""
import json
import sys

EXPECTED = (
    'crates/gateway-application/src/model_releases.rs',
    'crates/gateway-daemon/src/bounded_process.rs',
    'crates/gateway-daemon/src/cognitive_store.rs',
    'crates/gateway-daemon/src/cpu_learning.rs',
    'crates/gateway-daemon/src/durable_models.rs',
    'crates/gateway-daemon/src/durable_workers.rs',
    'crates/gateway-daemon/src/worker_fabric.rs',
)


def check(report):
    files = [f for data in report['data'] for f in data['files']]
    results = []
    for path in EXPECTED:
        matches = [f for f in files if f['filename'].replace('\\', '/').endswith('/' + path)]
        if len(matches) != 1:
            raise ValueError(f'Expected exactly one measured entry: {path}')
        lines = matches[0]['summary']['lines']
        count, covered = lines['count'], lines['covered']
        if type(count) is not int or type(covered) is not int or not 0 < count or not 0 <= covered <= count or 100 * covered < 95 * count:
            raise ValueError(f'Below 95% or invalid coverage: {path}: {covered}/{count}')
        results.append(f'{path}: {covered}/{count} ({100 * covered / count:.2f}%)')
    return results


if __name__ == '__main__':
    with open(sys.argv[1]) as source:
        print('\n'.join(check(json.load(source))))
