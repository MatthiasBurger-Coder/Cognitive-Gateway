#!/usr/bin/env bash
# Control only the local model service stack; model releases use models.sh.
set -euo pipefail
usage() {
  cat <<'HELP'
Usage: scripts/operations/local-model/service.sh start|stop|logs|ready
       scripts/operations/local-model/service.sh config [COMPOSE_CONFIG_OPTIONS...]

start: build/start the stack and wait up to 360 seconds for healthy services.
stop: stop the stack, retaining volumes. logs: show the last 100 log lines.
ready: GET /ready on 127.0.0.1:${CG_MODEL_SERVICE_PORT:-8091}, with a 10-second timeout.
config: render/validate Compose configuration without starting containers.
Requires Docker Compose; ready requires curl. Acceleration: CG_MODEL_ACCELERATION.
Exit 2: usage error; otherwise the invoked tool's status. Help performs no operation.
HELP
}
if [[ ${1:-} == --help || ${1:-} == -h || $# == 0 ]]; then usage; exit 0; fi
operation="$1"
shift
if [[ "$operation" != config && $# != 0 ]]; then
  echo 'Unexpected service arguments; see --help.' >&2
  exit 2
fi
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
case "$operation" in
  config) exec "$script_dir/compose.sh" config "$@" ;;
  start) exec "$script_dir/compose.sh" up -d --build --wait --wait-timeout 360 ;;
  stop) exec "$script_dir/compose.sh" stop ;;
  logs) exec "$script_dir/compose.sh" logs --tail 100 ;;
  ready) exec curl --fail --silent --max-time 10 "http://127.0.0.1:${CG_MODEL_SERVICE_PORT:-8091}/ready" ;;
  *) echo 'Unknown service operation; see --help.' >&2; exit 2 ;;
esac
