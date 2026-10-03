#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
files=(-f compose.model.yaml)
case ${CG_MODEL_ACCELERATION:-cpu} in
  cpu) ;;
  nvidia|gpu) files+=(-f compose.model.gpu.yaml) ;;
  amd) files+=(-f compose.model.amd.yaml) ;;
  vulkan) files+=(-f compose.model.vulkan.yaml) ;;
  *) echo 'CG_MODEL_ACCELERATION must be cpu, nvidia, amd or vulkan (gpu aliases nvidia)' >&2; exit 2 ;;
esac
case ${1:-help} in
  config) shift; docker compose "${files[@]}" config "$@" ;;
  start) docker compose "${files[@]}" up -d --build --wait --wait-timeout 360 ;;
  stop) docker compose "${files[@]}" stop ;;
  logs) docker compose "${files[@]}" logs --tail 100 ;;
  ready) curl --fail --silent --max-time 10 "http://127.0.0.1:${CG_MODEL_SERVICE_PORT:-8091}/ready" ;;
  install|test|promote|rollback|disable|inspect)
    docker compose "${files[@]}" exec -T model-service python service.py "$@" ;;
  *) echo 'Usage: scripts/model.sh config|start|stop|logs|ready|install /models/<profile>.json|test <id>|promote <id>|rollback <role>|disable <id>|inspect' ;;
esac
