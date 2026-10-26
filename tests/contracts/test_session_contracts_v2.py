"""Independent validation of the observable readonly preparation state."""
import copy
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]


class SessionReadonlyDispatchContract(unittest.TestCase):
    def test_reserved_dispatch_is_inspectable_through_both_public_snapshot_schemas(self):
        common = json.loads((ROOT / 'schemas/codex/v2/common.schema.json').read_text())
        response = json.loads((ROOT / 'schemas/codex/v2/response.schema.json').read_text())
        Draft202012Validator.check_schema(common)
        Draft202012Validator.check_schema(response)
        snapshot = dict(session='task', run='run', revision=2, state='dispatching',
                        pending=None, dispatch='observe-run-2',
                        dispatch_knowledge='reserved', command_outcome=None,
                        final_evidence=None)
        snapshot_schema = common['$defs']['snapshot']
        envelope = dict(schema_version='2.0',
                        scope=dict(workspace_id='workspace', project_id='project', binding_id='binding'),
                        operation='session.inspect', correlation=dict(request_id='inspect'), status='ok',
                        result=dict(session=snapshot, interaction=None,
                                    budget=dict(actions=1, retries=0, max_actions=2,
                                                max_retries=0, deadline_ms=300000)), diagnostics=[])
        Draft202012Validator(snapshot_schema).validate(snapshot)
        Draft202012Validator(response).validate(envelope)
        for state in ['preparing', 'preparation', 'provider_running', '', None]:
            bad = copy.deepcopy(envelope)
            bad['result']['session']['state'] = state
            self.assertFalse(Draft202012Validator(snapshot_schema).is_valid(bad['result']['session']))
            self.assertFalse(Draft202012Validator(response).is_valid(bad))
