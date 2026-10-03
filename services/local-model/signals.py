"""Replaceable classification, ranking, extraction and matching proposal adapter."""
import json
import math
from service import ModelError, digest, validate


class CognitiveSignalAdapter:
    def __init__(self, backend, profile, dataset):
        self.backend = backend
        self.profile = profile
        self.dataset = dataset

    def infer(self, task_name, value, cold=False):
        if task_name not in self.dataset['tasks'] or task_name not in self.profile['capabilities']:
            raise ModelError('task_unsupported')
        task = self.dataset['tasks'][task_name]
        if (self.dataset['input_contract'] not in self.profile['supported_input_contracts']
                or task['output_contract'] not in self.profile['supported_output_contracts']):
            raise ModelError('contract_unsupported')
        validate(task['input_schema'], value)
        request = {'schema_version': '1.0', 'role': self.profile['role'],
                   'input_contract': self.dataset['input_contract'], 'output_contract': task['output_contract'],
                   'output_schema': task['output_schema'],
                   'prompt': json.dumps({'task': task_name, 'input': value}, sort_keys=True)}
        self.backend.check(self.profile)
        result = self.backend.generate(self.profile, request, task['system'], cold=cold)
        validate(task['output_schema'], result['proposal'])
        if (result['kind'] != 'proposal' or result['model_id'] != self.profile['model_id']
                or result['artifact_digest'] != self.profile['artifact_digest']):
            raise ModelError('proposal_identity_invalid')
        provenance = result['provenance']
        if (any(provenance[key] != self.profile[key] for key in
                ('model_version', 'runtime', 'runtime_version', 'prompt_version', 'template_digest'))
                or provenance['system_digest'] != digest(task['system'])
                or provenance['input_contract'] != request['input_contract']
                or provenance['output_contract'] != request['output_contract']):
            raise ModelError('provenance_changed')
        if not isinstance(provenance['runtime_configuration'], dict):
            raise ModelError('provenance_changed')
        for key in ('latency_seconds', 'load_seconds', 'tokens_per_second'):
            metric = result['metrics'][key]
            if metric is None and key != 'latency_seconds':
                continue
            if isinstance(metric, bool) or not isinstance(metric, (int, float)) or not math.isfinite(metric) or metric < 0:
                raise ModelError('metrics_invalid')
        self.backend.check(self.profile)
        return result
