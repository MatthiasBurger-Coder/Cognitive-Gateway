#!/usr/bin/python3
"""Controlled GitHub HTTPS replacement for the shared document-read contract."""
import json
import sys
assert sys.stdin.read() == ''
args = sys.argv[1:]
assert args[args.index('--url') + 1] == 'https://api.github.com/repos/owner/repo/contents/README.md?ref=main'
sys.stdout.write(json.dumps({'name': 'README.md', 'path': 'README.md', 'type': 'file', 'sha': 'fixture-blob-v1', 'encoding': 'base64', 'content': 'U3ludGhldGljIGd1aWRlCg=='}) + '\n200')
