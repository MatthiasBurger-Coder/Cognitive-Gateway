"""Component evidence cannot pass with omitted gates or changed sources."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('qualification', ROOT / 'scripts/qualify-codex-local.py')
QUALIFICATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUALIFICATION)


class QualificationEvidenceTests(unittest.TestCase):
    def report(self):
        return {'revision': 'candidate', 'source_sha256': {'source': 'digest'},
                'gates': [{'name': name, 'command': command, 'exit_code': 0}
                          for name, command in QUALIFICATION.GATES]}

    def validate(self, report, sources=None):
        with patch.object(QUALIFICATION, 'fingerprint', return_value=sources or {'source': 'digest'}), \
             patch.object(QUALIFICATION.subprocess, 'check_output', return_value='candidate\n'):
            return QUALIFICATION.validate(report)

    def test_every_gate_is_required_and_failures_are_rejected(self):
        self.assertTrue(self.validate(self.report()))
        for change in ('missing', 'failed', 'command', 'order'):
            report = copy.deepcopy(self.report())
            if change == 'missing':
                report['gates'].pop()
            elif change == 'failed':
                report['gates'][0]['exit_code'] = 1
            elif change == 'command':
                report['gates'][0]['command'] = ['true']
            else:
                report['gates'].reverse()
            with self.assertRaises(ValueError):
                self.validate(report)

    def test_changed_source_or_revision_invalidates_evidence(self):
        with self.assertRaises(ValueError):
            self.validate(self.report(), {'source': 'changed'})
        report = self.report()
        report['revision'] = 'other'
        with self.assertRaises(ValueError):
            self.validate(report)
