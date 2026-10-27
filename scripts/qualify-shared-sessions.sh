#!/usr/bin/env bash
# Run through cognitive-test-host.py so database authority stays outside workers.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CG_COGNITIVE_TEST_DATABASE:?Use scripts/cognitive-test-host.py to supply the disposable PostgreSQL host}"
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
export CG_SESSION_EVIDENCE_DIR="$output"
: > "$output/transitions.jsonl"
export CG_QUALIFICATION_BIN_DIR="$PWD/target/session-shipped-cli/debug"
env -u LLVM_PROFILE_FILE \
  -u __CARGO_LLVM_COV_RUSTC_WRAPPER \
  -u __CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS \
  -u __CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES \
  -u RUSTC_WRAPPER \
  -u CARGO_LLVM_COV \
  -u CARGO_LLVM_COV_SHOW_ENV \
  CARGO_TARGET_DIR="$PWD/target/session-shipped-cli" \
  cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_LLVM_COV_TARGET_DIR=target/connector-session-coverage \
  cargo llvm-cov -p gateway-application --all-targets --locked -j1 --json \
    --output-path "$output/application-coverage.json"
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_LLVM_COV_TARGET_DIR=target/connector-daemon-coverage \
  cargo llvm-cov -p gateway-daemon --all-targets --locked -j1 --json \
    --output-path "$output/daemon-coverage.json"
python3 scripts/check-session-runtime-coverage.py "$output/application-coverage.json" "$output/daemon-coverage.json"
python3 -m unittest discover -s tests/codex-local -p test_sessions.py -v
