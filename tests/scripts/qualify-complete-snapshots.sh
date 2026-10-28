#!/usr/bin/env bash
# DATA/CONTRACT qualification; the actual MCP/PostgreSQL consumer is separate.
set -euo pipefail

# Purpose: Qualify complete scoped snapshot data/contracts; retain coverage evidence.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: tests/scripts/qualify-complete-snapshots.sh EVIDENCE_DIR

Qualify complete scoped snapshot data/contracts; retain coverage evidence.
Requires Bash, Python 3 and cargo-llvm-cov. Resets coverage build state; builds/runs tests.
Writes reports into EVIDENCE_DIR; reuse may overwrite existing evidence.
Exit 0: checks passed/help; nonzero: setup/check failure; 2: usage error.
HELP
  exit 0
fi
if [[ $# != 1 || -z "${1:-}" || "${1:-}" == -* ]]; then
  echo 'Invalid test runner arguments; see --help.' >&2
  exit 2
fi
cd "$(dirname "$0")/../.."
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/complete-snapshot-coverage}"
cargo llvm-cov clean --workspace
tests/scripts/with-coverage-env.sh cargo llvm-cov -p gateway-application --test complete_scoped_snapshots --locked \
  --json --output-path "$output/coverage.json"
python3 tests/scripts/check-complete-snapshot-coverage.py --self-test
python3 tests/scripts/check-complete-snapshot-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
