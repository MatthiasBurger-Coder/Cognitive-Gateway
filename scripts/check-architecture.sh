#!/usr/bin/env bash
# This script is intentionally LF-terminated for POSIX shells.
set -euo pipefail

fail() {
  echo "architecture violation: $*" >&2
  exit 1
}

DOMAIN="crates/gateway-domain/Cargo.toml"
APPLICATION="crates/gateway-application/Cargo.toml"

domain_dependencies=$(awk '
  /^\[dependencies\]/ { in_dependencies=1; next }
  /^\[/ { in_dependencies=0 }
  in_dependencies && $0 !~ /^[[:space:]]*(#|$)/ { print }
' "$DOMAIN")

if [[ -n "$domain_dependencies" ]] && printf '%s\n' "$domain_dependencies" \
  | grep -Eqv '^[[:space:]]*(serde|serde_json)(\.workspace)?[[:space:]]*='; then
  fail "gateway-domain may only declare serde serialization dependencies"
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
