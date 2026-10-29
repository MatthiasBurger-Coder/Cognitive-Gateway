#!/usr/bin/python3
"""Controlled curl substitute: validates secret/config plumbing; no network IO."""
import json
from pathlib import Path
import sys

args = sys.argv[1:]
config = sys.stdin.read()
assert args[0:3] == ['-q', '--config', '-']
assert 'unit-secret' not in ' '.join(args)
assert config in ('', 'header = "Authorization: Bearer unit-secret"\n')
url = args[args.index('--url') + 1]
assert url.startswith('https://api.github.com/')
if url.endswith('/failure'):
    sys.exit(1)
if url.endswith('/oversized'):
    print('x' * 100000)
elif url.endswith('/malformed'):
    sys.stdout.write('no-status')
elif url.endswith('/encoded-disclosure'):
    sys.stdout.write('{"body":"unit' + r'\u002d' + 'secret"}\n200')
elif url.endswith('/disclosure'):
    sys.stdout.write('unit-secret\n200')
elif '/http-' in url:
    sys.stdout.write('{}\n' + url.rsplit('-', 1)[1])
else:
    fixtures = json.loads(Path(__file__).with_name('responses.json').read_text())
    kind = 'issue' if '/issues/' in url else 'pull_request' if '/pulls/' in url else 'workflow' if '/actions/runs/' in url else 'repository'
    sys.stdout.write(json.dumps(fixtures[kind]) + '\n200')
