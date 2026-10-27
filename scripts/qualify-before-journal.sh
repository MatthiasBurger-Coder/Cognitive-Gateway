#!/usr/bin/env bash
# Component/native-data + real PostgreSQL and private operator qualification.
# Actual installed MCP capture is the separate mandatory #408 consumer.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CG_COGNITIVE_TEST_DATABASE:?Use scripts/cognitive-test-host.py to supply PostgreSQL}"
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CARGO_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/before-journal-coverage}"
cargo llvm-cov clean --workspace
eval "$(cargo llvm-cov show-env --sh)"
# Compile the identical test selection first, including the current real CLI
# binaries required by its integration targets, before any inspector test runs.
# Keep one Cargo profile/instrumentation mode throughout this fresh epoch.
cargo test -p gateway-application -p gateway-daemon --all-targets --no-run --locked
export CG_QUALIFICATION_BIN_DIR="$CARGO_TARGET_DIR/debug"
cargo test -p gateway-application -p gateway-daemon --all-targets --locked -- --test-threads=1
cargo llvm-cov report --json --output-path "$output/coverage.json"
python3 scripts/check-before-journal-coverage.py --self-test
python3 scripts/check-before-journal-coverage.py "$output/coverage.json" > "$output/coverage-counts.txt"
python3 scripts/check-complete-snapshot-coverage.py "$output/coverage.json" > "$output/snapshot-coverage-counts.txt"
