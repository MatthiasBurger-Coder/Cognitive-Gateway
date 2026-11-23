#!/usr/bin/env bash
set -euo pipefail

# Purpose: Start the persistent PostgreSQL service and run its Rust integration tests.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: tests/scripts/test-postgres.sh

Start the persistent PostgreSQL service and run its Rust integration tests.
Requires Bash and the Rust toolchain. Docker Compose is required.
Creates credentials if absent and starts persistent containers; test data is written.
Exit 0: checks passed/help; nonzero: setup/check failure; 2: usage error.
HELP
  exit 0
fi
if [[ $# != 0 ]]; then
  echo 'Invalid test runner arguments; see --help.' >&2
  exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
"$repo_root/operations/postgres/start.sh"
cargo test --manifest-path "$repo_root/Cargo.toml" -p gateway-daemon \
  --test postgres_experience -- --ignored
