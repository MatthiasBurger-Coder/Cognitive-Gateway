#!/usr/bin/env bash
set -euo pipefail

# Forward Docker Compose operations with the explicit PostgreSQL credential file.
if [[ ${1:-} == --help || ${1:-} == -h || $# == 0 ]]; then
  cat <<'HELP'
Usage: scripts/operations/postgres/compose.sh COMPOSE_ARGS...

Run Docker Compose for compose.yaml with the private PostgreSQL configuration.
CG_POSTGRES_ENV_FILE overrides ~/.config/cognitive-gateway/postgres.env (XDG_CONFIG_HOME honored).
Requires Docker Compose and an existing credential file; start.sh creates it if absent.
Effects and exit status: those of the requested Compose command; down retains data,
whereas down -v removes its volumes. No command runs on help. Missing configuration exits 1.
HELP
  exit 0
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
env_file="${CG_POSTGRES_ENV_FILE:-${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}/cognitive-gateway/postgres.env}"
if [[ ! -f "$env_file" ]]; then
  echo "Database configuration is missing: $env_file. Run scripts/operations/postgres/start.sh first." >&2
  exit 1
fi
exec docker compose --project-directory "$repo_root" --env-file "$env_file" \
  -f "$repo_root/compose.yaml" "$@"
