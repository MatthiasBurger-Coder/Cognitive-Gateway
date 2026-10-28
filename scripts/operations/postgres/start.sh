#!/usr/bin/env bash
set -euo pipefail

# Provision the PostgreSQL credential file if absent, then start the persistent service.
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: scripts/operations/postgres/start.sh

Create a private random-password file only if absent; validate and start PostgreSQL.
CG_POSTGRES_ENV_FILE overrides ~/.config/cognitive-gateway/postgres.env (XDG_CONFIG_HOME honored).
Requires Bash, Docker Compose and GNU stat; OpenSSL is required on first start.
Effects: creates credential directories/file, enforces file mode 600, starts the
persistent Compose service and waits for health. Existing credentials/data are retained.
Exit 0: ready/help; 1: dependency/configuration/service failure; 2: usage error.
HELP
  exit 0
fi
if (($#)); then echo 'Unexpected PostgreSQL start arguments; see --help.' >&2; exit 2; fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
env_file="${CG_POSTGRES_ENV_FILE:-${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}/cognitive-gateway/postgres.env}"

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

"$repo_root/scripts/operations/postgres/compose.sh" config --quiet
"$repo_root/scripts/operations/postgres/compose.sh" up -d --wait postgres
echo 'PostgreSQL is healthy. The named Docker volume retains its data across container recreation.'
