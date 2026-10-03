#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
config_root="${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}"
env_file="${CG_POSTGRES_ENV_FILE:-$config_root/cognitive-gateway/postgres.env}"
if [[ ! -f "$env_file" ]]; then
  echo "Database configuration is missing: $env_file. Run scripts/start-postgres.sh first." >&2
  exit 1
fi
exec docker compose --project-directory "$repo_root" --env-file "$env_file" \
  -f "$repo_root/compose.yaml" "$@"
