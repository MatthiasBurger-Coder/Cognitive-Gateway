#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
config_root="${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}"
env_file="${CG_POSTGRES_ENV_FILE:-$config_root/cognitive-gateway/postgres.env}"

if ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; then
  echo 'Docker with the Compose plugin is required.' >&2
  exit 1
fi
if [[ ! -f "$env_file" ]]; then
  if ! command -v openssl >/dev/null 2>&1; then
    echo 'OpenSSL is required to generate the initial database password.' >&2
    exit 1
  fi
  umask 077
  mkdir -p -- "$(dirname -- "$env_file")"
  password="$(openssl rand -hex 32)"
  printf 'CG_POSTGRES_PASSWORD=%s\n' "$password" > "$env_file"
  echo "Created $env_file with a random database password."
fi
chmod 600 "$env_file"
mode="$(stat -c '%a' "$env_file")"
if (( (8#$mode & 077) != 0 )); then
  echo "Database credential file is readable by others: $env_file" >&2
  exit 1
fi
if grep -qx 'CG_POSTGRES_PASSWORD=replace-with-a-long-random-password' "$env_file"; then
  echo "Replace the example database password in $env_file before starting PostgreSQL." >&2
  exit 1
fi

"$repo_root/scripts/postgres-compose.sh" config --quiet
"$repo_root/scripts/postgres-compose.sh" up -d --wait postgres
echo 'PostgreSQL is healthy. The named Docker volume retains its data across container recreation.'
