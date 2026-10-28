#!/usr/bin/env bash
# Requires the real PostgreSQL host supplied by the common qualification runner.
set -euo pipefail

# Purpose: Qualify observed-goal sessions with PostgreSQL; retain coverage evidence.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: tests/scripts/qualify-observed-goal-sessions.sh EVIDENCE_DIR

Qualify observed-goal sessions with PostgreSQL; retain coverage evidence.
Requires Bash, Python 3 and cargo-llvm-cov. Resets coverage build state; builds/runs tests.
Writes reports into EVIDENCE_DIR; reuse may overwrite existing evidence.
Run via tests/scripts/cognitive-test-host.py to supply CG_COGNITIVE_TEST_DATABASE.
Exit 0: checks passed/help; nonzero: setup/check failure; 2: usage error.
HELP
  exit 0
fi
if [[ $# != 1 || -z "${1:-}" || "${1:-}" == -* ]]; then
  echo 'Invalid test runner arguments; see --help.' >&2
  exit 2
fi
cd "$(dirname "$0")/../.."
: "${CG_COGNITIVE_TEST_DATABASE:?Use tests/scripts/cognitive-test-host.py to supply PostgreSQL}"
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/observed-goal-coverage}"
cargo llvm-cov clean --workspace
cargo llvm-cov -p gateway-application -p gateway-daemon --all-targets --locked \
  --json --output-path "$output/coverage.json" -- --test-threads=1
python3 tests/scripts/check-observed-goal-coverage.py --self-test
python3 tests/scripts/check-observed-goal-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
