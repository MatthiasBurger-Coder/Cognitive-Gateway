# ADR-022 — Outbound Connector Core and Adapter Ownership

- **Status:** Accepted
- **Date:** 2026-10-24
- **Scope:** EPIC-07.01 #224; elaborates ADR-008, ADR-010 and ADR-016.

## Context

The outbound MCP plane needs stable ports before manifests, protocol runtime,
provider mappings and authorization integration are implemented. Exposing wire
tools directly would confuse discovery with permission and authentication with
trust. Treating an acknowledgement as evidence of completion could duplicate
external effects after transport failures. The parent epic's 2026-10-04 review
also requires bindings to current policy, consent, inventory and aggregate limits.

## Decision

Adopt the normative [connector runtime architecture](../mcp-connector-runtime.md)
and `gateway_application::connectors` v1 port foundation. The application owns
driving operations, typed requests, admission and canonical validation; the outer
runtime implements the driven port with provider adapters and replaceable
transports. No SDK, wire envelope, credential, HTTP or process type enters the
core. Runtime implementation is composed in the outer daemon and cannot be
called by models or inbound clients directly.

CG owns planning, process legality, capability mapping, scope, current policy,
consent, budget and retry decisions, evidence trust and completion. MCP owns
protocol negotiation, discovery/session mechanics and invocation delivery.
Provider adapters translate configured schemas and normalize candidate lineage;
transport adapters handle frames/connections and secret-bearing authentication.
The target protocol profile pins MCP 2025-11-25; optional extensions and other
versions/transports require explicit admission and qualification.

READ/SEARCH/INSPECT map to existing Inspect; MUTATE/ADMIN map to Mutate, with
ADMIN requiring its own explicit capability/policy. Every dispatch binds scope,
task/step/execution, typed arguments/resource, inventory, current policy/consent
and durable aggregate budget reservation. Public records are not cryptographic
grants and require runtime enforcement in #230. Results reuse CG-06 lineage;
acknowledgements and independent post-effect observations remain distinct.
Unknown mutation outcomes block retries absent explicit safe reconciliation.

## Alternatives considered

- Put MCP/provider types in domain contracts: rejected because all inner consumers
  would depend on a wire version and provider vocabulary.
- Give each provider its own planner/policy/normalizer authority: rejected because
  this duplicates CG services and makes authorization inconsistent.
- Share inbound Codex and outbound connector authority/credentials: rejected by
  ADR-016; directions and trust boundaries are distinct.
- Deliver documentation without compilable interfaces: rejected because later
  slices could implement incompatible application/runtime boundaries unnoticed.

## Consequences and review

Transport or provider changes do not require domain changes. The existing Cargo
DAG remains unchanged. Protocol compatibility, concrete manifests, credential
resolution, authorization enforcement and normalization remain separate child
implementations; this ADR does not claim those behaviors exist.

The architecture document contains the responsibility matrix, Mermaid component
and data-flow diagrams, negative coupling examples, compatibility profile,
acceptance traceability and completed single-agent review checklist. Compile
fixtures and dependency regression tests qualify the foundation; live MCP and
external effects are deferred to the runtime and qualification slices.

## EPIC-07.05 contract revision — 2026-10-24

Port revision 2 intentionally replaces the v1 foundation's observation record with
mandatory [canonical result metadata](../connector-result-v1.md). The driving and
driven method signatures and ownership remain the same. Manifest format 1.0 now
requires port revision 2; old manifests must be regenerated and inventories
rediscovered. Every host result reuses the application normalization validator;
CG-06 ingestion and reference-only context adapters reject acknowledgements.
Authentication, current policy and goal completion remain their existing owners.

## EPIC-07.06 credential isolation — 2026-10-25

Credentials are resolved by an outer `ConnectorSecretResolver` from an opaque
`CredentialProfileRef`. The resolved value has no serialization/debug contract.
`CredentialedStdioFactory` resolves on each connection, binds the result only to
the configured connector and server, and gates each mapped tool against the
profile allowlist. Unprofiled authentication environment keys are rejected.
Provider results containing a resolved secret are denied before mapping or
normalization. Rotation takes effect on an admitted reconnect; no domain record
is mutated. Resolver/provider diagnostics are projected to fixed failure codes.
This qualifies the component boundary and synthetic test adapter; it does not
claim live provider or installed-binary qualification.
