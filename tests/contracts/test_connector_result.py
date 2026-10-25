"""Independent scalar normalization fixture digest and stale/conflict vector."""
import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ConnectorResultFixtureTests(unittest.TestCase):
    def test_independent_content_digest_and_freshness_vector(self):
        fixture = json.loads((ROOT / 'tests/fixtures/connector-result-v1/cases.json').read_text())
        payload = json.dumps(fixture['valid'], sort_keys=True, separators=(',', ':')).encode()
        self.assertEqual(hashlib.sha256(payload).hexdigest(), fixture['content_digest'])
        stale = fixture['stale_conflict']
        self.assertGreater(stale['evaluated_at'] - stale['observed_at'], stale['max_age_seconds'])
        self.assertEqual(stale['conflict'], 'UNRESOLVED')
        self.assertTrue(stale['partial'])
        self.assertTrue(fixture['malformed'])
