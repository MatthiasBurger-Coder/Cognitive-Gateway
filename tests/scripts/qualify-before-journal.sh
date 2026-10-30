#!/usr/bin/env bash
# Component/native-data + real PostgreSQL and private operator qualification.
# Actual installed MCP capture is the separate mandatory #408 consumer.
set -euo pipefail

# Purpose: Qualify Before journal persistence and historical inspection; retain coverage evidence.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: tests/scripts/qualify-before-journal.sh EVIDENCE_DIR

Qualify Before journal persistence and historical inspection; retain coverage evidence.
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
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/before-journal-coverage}"
export CARGO_TARGET_DIR="$CARGO_LLVM_COV_TARGET_DIR"
cargo llvm-cov clean --workspace
eval "$(cargo llvm-cov show-env --sh)"
# Compile the identical test selection first, including the current real CLI
# binaries required by its integration targets, before any inspector test runs.
# Keep one Cargo profile/instrumentation mode throughout this fresh epoch.
cargo test -p gateway-application -p gateway-daemon --all-targets --no-run --locked
export CG_QUALIFICATION_BIN_DIR="$CARGO_TARGET_DIR/debug"
cargo test -p gateway-application -p gateway-daemon --all-targets --locked -- --test-threads=1
cargo llvm-cov report --json --output-path "$output/coverage.json"
python3 tests/scripts/check-before-journal-coverage.py --self-test
python3 tests/scripts/check-before-journal-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
python3 tests/scripts/check-complete-snapshot-coverage.py "$output/coverage.json" > "$output/snapshot-coverage-counts.txt"
