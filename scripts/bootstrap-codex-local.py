#!/usr/bin/env python3
"""Create an isolated operator example and check it; never edit Codex user settings."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, 'CG_BOOTSTRAP_FAILED: use --help; supply required options without credential fields\n')


def main():
    parser = SafeParser(description=__doc__)
    parser.add_argument('--repository', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True,
                        help='New directory; existing files are never overwritten')
    parser.add_argument('--client-name', required=True, help='Exact initialize clientInfo.name')
    parser.add_argument('--client-version', required=True, help='Exact initialize clientInfo.version')
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    args = parser.parse_args()
    try:
        repository = args.repository.resolve(strict=True)
        binary = (args.bin_dir / 'cg-mcp').resolve(strict=True)
        cli = (args.bin_dir / 'cg-local').resolve(strict=True)
        env = shutil.which('env')
        if not repository.is_dir() or not env or not all(os.access(p, os.X_OK) for p in (binary, cli)):
            raise ValueError()
        output = args.output.resolve()
        output.mkdir(mode=0o700, parents=True, exist_ok=False)
        config = json.loads((ROOT / 'examples/codex-local/admission.example.json').read_text())
        mapping = config['mappings'][0]
        mapping['repository'] = str(repository)
        request = json.loads((ROOT / 'examples/codex-local/situation.inspect.request.json').read_text())
        document = request['input']['situation']['document']
        canonical = json.dumps(document, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
        reference = {'id': 'operator-smoke', 'contract': 'cg.situation', 'contract_version': '1.0',
                     'revision': '1', 'digest': 'sha256:' + hashlib.sha256(canonical.encode()).hexdigest()}
        mapping['resources'] = [{'schema_version': '1.0', 'scope': mapping['scope'],
                                'reference': reference, 'document': document,
                                'provenance': [{'reference': reference, 'source_id': 'synthetic-operator-smoke',
                                                'source_revision': '1', 'freshness': 'current',
                                                'sensitivity': 'PUBLIC', 'lineage': []}]}]
        admission = output / 'admission.json'
        admission.write_text(json.dumps(config, indent=2) + '\n')
        request['input']['situation'] = {'kind': 'reference', 'reference': reference}
        (output / 'request.json').write_text(json.dumps(request, indent=2) + '\n')
        launch = ['--client-name', args.client_name, '--client-version', args.client_version,
                  '--principal', mapping['principal'], '--workspace', mapping['scope']['workspace_id'],
                  '--project', mapping['scope']['project_id'], '--binding', mapping['scope']['binding_id'],
                  '--admission', str(admission), '--cwd', str(repository), '--repository', str(repository),
                  '--session', mapping['session_id']]
        result = subprocess.run([str(cli), '--check'] + launch, env={}, capture_output=True,
                                text=True, timeout=10, check=False, cwd=cli.parent)
        if result.returncode:
            sys.stderr.write(result.stderr)
            return 2
        # /usr/bin/env -i is the actual boundary: env_vars=[] alone need not clear defaults.
        toml = '[mcp_servers.cognitive_gateway]\ncommand = ' + json.dumps(env) + '\n'
        toml += 'args = ' + json.dumps(['-i', str(binary)] + launch) + '\n'
        toml += 'env_vars = []\nstartup_timeout_sec = 10\ntool_timeout_sec = 35\n'
        toml += 'enabled_tools = ["cg_situation_inspect_v1", "cg_situation_assess_v1"]\n'
        (output / 'config.toml').write_text(toml)
        (output / 'launch.json').write_text(json.dumps(launch, indent=2) + '\n')
        print(result.stdout.strip())
        return 0
    except (OSError, ValueError, subprocess.TimeoutExpired):
        print('CG_BOOTSTRAP_FAILED: build cg-mcp/cg-local; use an existing repository and a new writable output directory', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
