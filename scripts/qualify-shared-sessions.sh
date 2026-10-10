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
export CARGO_TARGET_DIR="$PWD/target/session-coverage"
cargo llvm-cov clean --workspace
eval "$(cargo llvm-cov show-env --sh)"
cargo test -p gateway-application -p gateway-daemon --lib --bin cg --bin cg-mcp --bin cg-local \
  --test session_contracts --test session_runtime --test local_mcp --test codex_facade --test codex_isolation \
  --test codex_local_cli --test codex_qualification --test codex_canonical --test declarative_cli --locked
cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
export CG_QUALIFICATION_BIN_DIR="$CARGO_TARGET_DIR/debug"
python3 -m unittest discover -s tests/codex-local -p test_sessions.py -v
cargo llvm-cov report --json --output-path "$output/coverage.json"
python3 scripts/check-session-runtime-coverage.py "$output/coverage.json"
