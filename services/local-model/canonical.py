"""Canonical transport bridge. Rust retains admission and output verification.

Legacy qualification is checked for runtime identity only. Supported contracts
must additionally be declared; readiness never adds SemanticTaskIR support.
"""

def invoke(registry, request, error_type):
    # The entrypoint supplies its error class; importing service here would
    # create a different class when service.py runs as __main__.

    def require(condition, code='request_invalid'):
        if not condition:
            raise error_type(code)

    require(request.get('schema_version') == '1.0')
    profile = registry.profile(request.get('model_id'))
    require(profile['lifecycle'] == 'active', 'active_model_missing')
    registry.qualified(profile)
    require(request.get('artifact_digest') == profile['artifact_digest']
            and request.get('model_version') == profile['model_version'], 'provenance_changed')
    require(request.get('input_contract') in profile['supported_input_contracts']
            and request.get('output_contract') in profile['supported_output_contracts'], 'contract_unsupported')
    messages = request.get('messages')
    require(isinstance(messages, list) and len(messages) == 2)
    require([m.get('role') for m in messages if isinstance(m, dict)] == ['system', 'user'])
    require(all(isinstance(m.get('content'), str) and m['content'] for m in messages))
    maximum = request.get('max_output_tokens')
    context = request.get('context_limit')
    require(type(maximum) is int and type(context) is int
            and 0 < maximum < context <= profile['context_limit'])
    options = request.get('options')
    require(isinstance(options, dict) and set(options) <= {'temperature', 'top_p', 'top_k', 'stop', 'seed'})
    for name, value in options.items():
        if name in ('temperature', 'top_p'):
            require(type(value) in (int, float) and (0 <= value <= 2 if name == 'temperature' else 0 < value <= 1))
        elif name in ('top_k', 'seed'):
            require(type(value) is int and value >= (1 if name == 'top_k' else 0))
        else:
            require(isinstance(value, list) and value and all(isinstance(s, str) and s for s in value))
    mode = request.get('structured_output_mode')
    require(mode in ('JSON', 'JSON_SCHEMA'), 'contract_unsupported')
    schema = request.get('output_schema')
    require(isinstance(schema, dict))
    options = {**options, 'num_predict': maximum, 'num_ctx': context}
    if registry.runtime.acceleration == 'cpu':
        options['num_gpu'] = 0
    result = registry.runtime.call('/api/chat', {
        'model': profile['runtime_model'], 'messages': messages, 'stream': False,
        'think': False, 'format': schema if mode == 'JSON_SCHEMA' else 'json', 'options': options,
    })
    require(result.get('model') == profile['runtime_model'], 'provenance_changed')
    require(result.get('done') is True and result.get('done_reason') == 'stop', 'output_incomplete')
    message = result.get('message', {})
    require(isinstance(message, dict) and isinstance(message.get('content'), str)
            and not message.get('tool_calls'), 'output_invalid')
    # Recheck immutable runtime provenance after generation as well as before.
    registry.runtime.check(profile)
    response = {'schema_version': '1.0', 'kind': 'proposal', 'model_id': profile['model_id'],
                'model_version': profile['model_version'], 'artifact_digest': profile['artifact_digest'],
                'content': message['content']}
    if 'prompt_eval_count' in result and 'eval_count' in result:
        require(all(type(result[k]) is int and result[k] >= 0 for k in ('prompt_eval_count', 'eval_count')),
                'runtime_response_invalid')
        response['usage'] = {'prompt_tokens': result['prompt_eval_count'], 'completion_tokens': result['eval_count']}
    return response
