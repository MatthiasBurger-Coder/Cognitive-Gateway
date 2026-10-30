#!/usr/bin/env bash
# Select the configured acceleration overlay and forward arguments to Docker Compose.
set -euo pipefail
if [[ ${1:-} == --help || ${1:-} == -h || $# == 0 ]]; then
  cat <<'HELP'
Usage: scripts/operations/local-model/compose.sh COMPOSE_ARGS...

Run Docker Compose for the local model stack from any working directory.
CG_MODEL_ACCELERATION: cpu (default), nvidia/gpu, amd or vulkan selects the overlay.
Other CG_MODEL_* settings are interpreted by the Compose files.
Effects and exit status: those of the requested Compose command. No command runs on help.
HELP
  exit 0
fi
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
files=(-f "$repo_root/compose.model.yaml")
case ${CG_MODEL_ACCELERATION:-cpu} in
  cpu) ;;
  nvidia|gpu) files+=(-f "$repo_root/compose.model.gpu.yaml") ;;
  amd) files+=(-f "$repo_root/compose.model.amd.yaml") ;;
  vulkan) files+=(-f "$repo_root/compose.model.vulkan.yaml") ;;
  *) echo 'CG_MODEL_ACCELERATION must be cpu, nvidia, amd or vulkan (gpu aliases nvidia).' >&2; exit 2 ;;
esac
exec docker compose --project-directory "$repo_root" "${files[@]}" "$@"
