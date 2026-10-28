#!/usr/bin/env bash
# Delegate model lifecycle commands to the running model-service container.
set -euo pipefail
usage() {
  cat <<'HELP'
Usage: scripts/operations/local-model/models.sh inspect
       scripts/operations/local-model/models.sh install /models/PROFILE.json
       scripts/operations/local-model/models.sh test|promote|disable MODEL_ID
       scripts/operations/local-model/models.sh rollback ROLE

Requires the already running model-service and Docker Compose.
install: download the model and register a pinned artifact snapshot (profile path is inside
the container). test: execute inference qualification. promote/rollback/disable: change
release state/aliases. inspect: read registry state. Service policy owns these decisions.
Effects and exit status: those of the model-service command; no stack is started.
Exit 2: usage error. Acceleration overlay: CG_MODEL_ACCELERATION. Help performs no operation.
HELP
}
if [[ ${1:-} == --help || ${1:-} == -h || $# == 0 ]]; then usage; exit 0; fi
case "$1" in
  inspect) expected=1 ;;
  install|test|promote|rollback|disable) expected=2 ;;
  *) echo 'Unknown model operation; see --help.' >&2; exit 2 ;;
esac
if [[ $# != "$expected" || ( "$expected" == 2 && -z "$2" ) ]]; then
  echo 'Invalid model arguments; see --help.' >&2
  exit 2
fi
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
exec "$script_dir/compose.sh" exec -T model-service python service.py "$@"
