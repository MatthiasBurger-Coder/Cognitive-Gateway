#!/usr/bin/python3
"""Independent wire peer: controlled protocol/schema faults, never production IO."""
import json
from pathlib import Path
import sys

mode = sys.argv[1]
bundle = json.loads(Path(__file__).with_name('standard.json').read_text())
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'initialize':
        result = {'protocolVersion': 'wrong' if mode == 'wrong-version' else '2025-11-25',
                  'serverInfo': {'name': 'independent-peer', 'version': '1.0.0'}, 'capabilities': {'tools': {}}}
    elif method == 'tools/list':
        result = {'tools': bundle['discovery']['tools']}
        tool = result['tools'][0]
        if mode == 'nested-schema':
            tool['inputSchema']['properties']['key'] = {'type': 'object', 'properties': {}}
        elif mode == 'open-schema':
            tool['inputSchema']['additionalProperties'] = True
        elif mode == 'schema-drift':
            tool['description'] = 'changed inventory'
        elif mode == 'unknown-tool':
            tool['name'] = 'unmapped.fetch'
        elif mode == 'duplicate-tool':
            result['tools'].append(tool.copy())
    elif method == 'tools/call':
        data = {'uri': 'conformance://documents/guide', 'body': 'Controlled document', 'version': 'fixture-v1'}
        if mode == 'bad-result':
            data['body'] = {'nested': 'unsupported'}
        elif mode == 'wrong-object':
            data['uri'] = 'conformance://foreign/guide'
        result = {'content': [], 'structuredContent': data, 'isError': False}
        if mode == 'tool-error':
            result = {'content': [{'type': 'text', 'text': 'DOCUMENT_NOT_FOUND'}], 'isError': True}
        if mode == 'malformed-json':
            print('invalid JSON', flush=True)
            continue
    else:
        result = {}
    response = {'jsonrpc': '2.0', 'id': request['id'], 'result': result}
    if method == 'tools/call' and mode == 'wrong-id':
        response['id'] += 100
    if method == 'tools/call' and mode == 'rpc-error':
        response.pop('result')
        response['error'] = {'code': -32603, 'message': 'private-provider-text'}
    print(json.dumps(response), flush=True)
