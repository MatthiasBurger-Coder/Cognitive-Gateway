#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
"$repo_root/scripts/start-postgres.sh"
cargo test --manifest-path "$repo_root/Cargo.toml" -p gateway-daemon \
  --test postgres_experience -- --ignored
