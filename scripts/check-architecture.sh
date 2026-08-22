#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "architecture violation: $*" >&2
  exit 1
}

DOMAIN="crates/gateway-domain/Cargo.toml"
APPLICATION="crates/gateway-application/Cargo.toml"

if grep -Eq '^\[dependencies\]' "$DOMAIN"; then
  fail "gateway-domain must not declare outward dependencies"
fi

for manifest in "$DOMAIN" "$APPLICATION"; do
  if grep -Eiq '(openai|anthropic|ollama|praison|codex|langchain|llamaindex|qdrant|weaviate|pinecone|chroma|neo4j|mcp|github)' "$manifest"; then
    fail "forbidden infrastructure dependency in $manifest"
  fi
done

for manifest in crates/*/Cargo.toml; do
  if grep -Eq 'gateway-daemon.*path' "$manifest" && [[ "$manifest" != "crates/gateway-daemon/Cargo.toml" ]]; then
    fail "inner crate depends on gateway-daemon: $manifest"
  fi
done

echo "architecture dependency guard passed"
