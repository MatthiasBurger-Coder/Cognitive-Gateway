#!/usr/bin/env python3
"""Compare admitted CLI and MCP smoke results without Codex or provider credentials."""
import argparse
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading

ROOT = Path(__file__).resolve().parent.parent


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, 'CG_SMOKE_FAILED: use --help; supply setup and binary directories\n')


def main():
    parser = SafeParser(description=__doc__)
    parser.add_argument('--setup', type=Path, required=True)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    args = parser.parse_args()
    process = None
    try:
        launch = json.loads((args.setup / 'launch.json').read_text())
        request = json.loads((args.setup / 'request.json').read_text())
        cli = (args.bin_dir / 'cg-local').resolve()
        checked = subprocess.run([str(cli), '--check'] + launch, env={}, capture_output=True,
                                 text=True, timeout=10, check=True, cwd=cli.parent)
        health = json.loads(checked.stdout)
        called = subprocess.run([str(cli), '--operation', 'situation.inspect', '--request',
                                 str((args.setup / 'request.json').resolve())] + launch, env={},
                                capture_output=True, text=True, timeout=35, check=True, cwd=cli.parent)
        result = json.loads(called.stdout)
        process = subprocess.Popen([str((args.bin_dir / 'cg-mcp').resolve())] + launch,
                                   env={}, text=True, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=cli.parent)
        replies = queue.Queue()
        def reader():
            for line in process.stdout:
                replies.put(line)
            replies.put(None)
        threading.Thread(target=reader, daemon=True).start()
        def exchange(frame):
            process.stdin.write(json.dumps(frame) + '\n')
            process.stdin.flush()
            if 'id' in frame:
                line = replies.get(timeout=35)
                if line is None:
                    raise ValueError()
                reply = json.loads(line)
                if reply.get('id') != frame['id'] or 'error' in reply:
                    raise ValueError()
                return reply['result']
        initialized = exchange({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
            'protocolVersion': health['mcp_protocol_version'], 'capabilities': {},
            'clientInfo': {'name': health['client_name'], 'version': health['client_version']}}})
        exchange({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        discovery = exchange({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'})
        mcp = exchange({'jsonrpc': '2.0', 'id': 3, 'method': 'tools/call', 'params': {
            'name': 'cg_situation_inspect_v1', 'arguments': request}})
        if (initialized['protocolVersion'] != health['mcp_protocol_version']
                or len(discovery['tools']) != 13 or result['status'] != 'ok'
                or mcp['structuredContent'] != result or mcp['isError']
                or json.loads(mcp['content'][0]['text']) != result):
            raise ValueError()
        process.stdin.close()
        if process.wait(timeout=10) != 0:
            raise ValueError()
        print('Codex local smoke passed: protocol 2025-11-25, 13 tools, admitted scope, CLI/MCP results identical, no provider environment')
        return 0
    except (OSError, ValueError, KeyError, TypeError, queue.Empty, subprocess.SubprocessError):
        print('CG_SMOKE_FAILED: run cg-local --check; verify setup, binaries and supported versions', file=sys.stderr)
        return 2
    finally:
        if process is not None:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()


if __name__ == '__main__':
    sys.exit(main())
