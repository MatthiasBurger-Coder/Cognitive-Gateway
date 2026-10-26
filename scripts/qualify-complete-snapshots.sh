#!/usr/bin/env bash
# DATA/CONTRACT qualification; the actual MCP/PostgreSQL consumer is separate.
set -euo pipefail
cd "$(dirname "$0")/.."
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/complete-snapshot-coverage}"
cargo llvm-cov clean --workspace
cargo llvm-cov -p gateway-application --test complete_scoped_snapshots --locked \
  --json --output-path "$output/coverage.json"
python3 scripts/check-complete-snapshot-coverage.py --self-test
python3 scripts/check-complete-snapshot-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
