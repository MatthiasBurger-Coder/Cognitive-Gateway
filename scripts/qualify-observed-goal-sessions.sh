#!/usr/bin/env bash
# Requires the real PostgreSQL host supplied by the common qualification runner.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CG_COGNITIVE_TEST_DATABASE:?Use scripts/cognitive-test-host.py to supply PostgreSQL}"
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/observed-goal-coverage}"
cargo llvm-cov clean --workspace
cargo llvm-cov -p gateway-application -p gateway-daemon --all-targets --locked \
  --json --output-path "$output/coverage.json" -- --test-threads=1
python3 scripts/check-observed-goal-coverage.py --self-test
python3 scripts/check-observed-goal-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
