#!/usr/bin/env python3
"""Run a command against a disposable, loopback-only PostgreSQL test host."""
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time
import uuid


class CognitiveTestHost:
    def __enter__(self):
        self.name = 'cg-epic03-test-' + uuid.uuid4().hex[:12]
        self.password = secrets.token_hex(24)
        env = {**os.environ, 'POSTGRES_PASSWORD': self.password}
        with socket.socket() as port_socket:
            port_socket.bind(('127.0.0.1', 0))
            host_port = port_socket.getsockname()[1]
        try:
            subprocess.run(['docker', 'run', '--detach', '--name', self.name,
                            '-e', 'POSTGRES_USER=cg', '-e', 'POSTGRES_DB=cg', '-e', 'POSTGRES_PASSWORD',
                            '-p', f'127.0.0.1:{host_port}:5432', 'postgres:16'], env=env, check=True, capture_output=True)
            port = subprocess.check_output(['docker', 'port', self.name, '5432/tcp'], text=True).strip().rsplit(':', 1)[1]
            for _ in range(60):
                ready = subprocess.run(['docker', 'exec', self.name, 'pg_isready', '-U', 'cg', '-d', 'cg'], capture_output=True)
                if ready.returncode == 0:
                    break
                time.sleep(0.5)
            else:
                raise RuntimeError('PostgreSQL test host did not become ready')
            self.environment = {'CG_COGNITIVE_TEST_DATABASE': f'host=127.0.0.1 port={port} user=cg password={self.password} dbname=cg',
                                'CG_COGNITIVE_TEST_CONTAINER': self.name, 'RUST_TEST_THREADS': '1'}
            self.report = {'schema_version': 1, 'host': 'disposable-loopback-postgresql',
                           'postgres_image': subprocess.check_output(['docker', 'inspect', '--format', '{{.Image}}', self.name], text=True).strip(),
                           'persistence': 'postgres-transactional-journals', 'authority_credentials_in_workers': False}
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def __exit__(self, *unused):
        subprocess.run(['docker', 'rm', '--force', '--volumes', self.name], capture_output=True)


if __name__ == '__main__':
    with CognitiveTestHost() as host:
        command = sys.argv[1:]
        if command[:1] == ['--']:
            command = command[1:]
        if not command:
            raise SystemExit('command required')
        raise SystemExit(subprocess.run(command, env={**os.environ, **host.environment}).returncode)
