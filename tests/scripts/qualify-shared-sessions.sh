#!/usr/bin/env bash
# Run through cognitive-test-host.py so database authority stays outside workers.
set -euo pipefail

# Purpose: Qualify shared-session runtime and CLI/MCP parity with PostgreSQL.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: tests/scripts/qualify-shared-sessions.sh EVIDENCE_DIR

Qualify shared-session runtime and CLI/MCP parity with PostgreSQL.
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
: "${CG_COGNITIVE_TEST_DATABASE:?Use tests/scripts/cognitive-test-host.py to supply the disposable PostgreSQL host}"
output="${1:?Supply an evidence directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
# Native complete-Before reads must run within the unchanged real deadlines.
# Keep the Rust tests and the subsequent private CLI checks in one profile.
export CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_TEST_OPT_LEVEL=1
source tests/scripts/with-coverage-env.sh
export CG_SESSION_EVIDENCE_DIR="$output"
: > "$output/transitions.jsonl"
export CARGO_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$PWD/target/session-coverage}"
cargo llvm-cov clean --workspace
eval "$(cargo llvm-cov show-env --sh)"
cargo test -p gateway-application -p gateway-daemon --lib --bin cg --bin cg-mcp --bin cg-local \
  --test session_contracts --test session_runtime --test context_application --test local_mcp --test codex_facade --test codex_isolation \
  --test codex_local_cli --test codex_qualification --test codex_canonical --test declarative_cli \
  --test observed_goal_sessions --test complete_before_sessions --locked -- --test-threads=1
cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
export CG_QUALIFICATION_BIN_DIR="${CARGO_TARGET_DIR:-$CARGO_LLVM_COV_TARGET_DIR}/debug"
python3 -m unittest discover -s tests/codex-local -p test_sessions.py -v
cargo llvm-cov report --json --output-path "$output/coverage.json"
python3 tests/scripts/check-session-runtime-coverage.py "$output/coverage.json"
