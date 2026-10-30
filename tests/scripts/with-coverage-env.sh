#!/usr/bin/env bash
# Purpose: Retain real function bodies for reliable optimized coverage reports.
# Source this file to configure the current shell, or execute it with a command.

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  set -euo pipefail
  if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    cat <<'HELP'
Usage: tests/scripts/with-coverage-env.sh COMMAND [ARG...]

Apply Rust coverage compiler flags and execute COMMAND with its arguments.
Source this file instead to apply the flags to the current Bash shell.
Preserves existing compiler flags and Cargo's encoded-flags precedence.
Sourcing starts no commands; execution starts only the supplied command.
Exit 0: help; 2: usage error; otherwise the command's exit status.
HELP
    exit 0
  fi
  if [[ $# == 0 || $1 == -* ]]; then
    echo 'Supply a command; see --help.' >&2
    exit 2
  fi
fi

# An explicitly empty encoded-flags setting also takes precedence over RUSTFLAGS.
if [[ ${CARGO_ENCODED_RUSTFLAGS+set} == set ]]; then
  if [[ -n $CARGO_ENCODED_RUSTFLAGS ]]; then
    CARGO_ENCODED_RUSTFLAGS+=$'\x1f'
  fi
  CARGO_ENCODED_RUSTFLAGS+=$'-C\x1flink-dead-code'
  export CARGO_ENCODED_RUSTFLAGS
else
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-dead-code"
fi

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  exec "$@"
fi
