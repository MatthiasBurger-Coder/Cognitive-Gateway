#!/usr/bin/env python3
"""Opt-in MCP peer performing actual GitHub ref IO; never returns fixture data."""
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def main():
    bundle, repository, branch, mode, receipt = sys.argv[1:]
    if (len(repository.split('/')) != 2 or not all(
            c.isalnum() or c in '-_./' for c in repository)
            or not branch.startswith('cg-qualification-235-')
            or not all(c.isalnum() or c == '-' for c in branch)
            or mode not in ('ack', 'lost-response')):
        return 2
    discovery = json.loads(Path(bundle).read_text())['discovery']
    base = 'https://api.github.com/repos/' + repository
    endpoint = base + '/git/ref/heads/' + urllib.parse.quote(branch, safe='')
    opener = urllib.request.build_opener(NoRedirect)
    token = os.environ['GITHUB_PERSONAL_ACCESS_TOKEN']

    def api(method, url, data=None):
        request = urllib.request.Request(url, method=method,
            data=json.dumps(data).encode() if data is not None else None,
            headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                     'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json',
                     'User-Agent': 'cg-connector-live-qualification'})
        with opener.open(request, timeout=20) as response:
            return json.load(response)

    for line in sys.stdin:
        message = json.loads(line)
        if 'id' not in message:
            continue
        method = message['method']
        if method == 'initialize':
            result = {'protocolVersion': '2025-11-25', 'capabilities': {'tools': {}},
                      'serverInfo': {'name': 'qualification.github.live', 'version': '1.0.0'}}
        elif method == 'tools/list':
            result = {'tools': discovery['tools']}
        elif method == 'ping':
            result = {}
        elif method == 'tools/call':
            params = message['params']
            args = params['arguments']
            if args.get('ref') != 'refs/heads/' + branch:
                raise ValueError('Target differs from the admitted isolated branch')
            if params['name'] == 'refs.create':
                response = api('POST', base + '/git/refs', {'ref': args['ref'], 'sha': args['sha']})
                # Test cleanup ownership only; this receipt is never goal evidence.
                Path(receipt).write_text(json.dumps({'branch': branch, 'sha': response['object']['sha']}))
                if mode == 'lost-response':
                    return 0
            elif params['name'] == 'refs.read':
                response = api('GET', endpoint)
            else:
                raise ValueError('Unknown tool')
            result = {'content': [], 'structuredContent': {
                'uri': endpoint, 'sha': response['object']['sha'], 'exists': True}, 'isError': False}
        else:
            raise ValueError('Unsupported method')
        print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, urllib.error.URLError):
        # Remote bodies, headers and credentials never become MCP diagnostics.
        raise SystemExit(1)
