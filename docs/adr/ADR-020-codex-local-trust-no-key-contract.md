# ADR-020 — Codex Local Trust Boundary and No-Key Contract

- **Status:** Accepted
- **Date:** 2026-10-04
- **Scope:** EPIC-04.01 #236, extending ADR-008 and ADR-016

## Context

Codex needs to consume CG capabilities locally without CG owning provider
credentials. Local transport and discovery do not prove authority. An ambiguous
boundary could leak secrets, conflate workspace scopes or bypass CG policy.

## Decision

Adopt the normative [local integration contract](../codex-local-integration.md).
Codex -> CG uses an outer driving adapter, initially private stdio. Codex owns
its account/session authentication. Trusted local launch and CG configuration
bind the principal, admitted client/version and workspace/project/session.
Client-supplied identifiers are claims, not authentication. Invalid identities,
versions, scope or operations fail closed before application dispatch.

CG never receives, requires, stores or relays an OpenAI API key or Codex session
credential for this path. Launch configuration isolates provider credentials; CG
must not read Codex auth stores or perform provider authentication fallback.

Default application authority is read-only, subject to current CG policy and
sensitivity rules. Mutations require existing authorization, trusted consent,
evidence and process gates. Administration is not initially exposed. Discovery,
client approval settings and model output grant no authority.

The adapter translates protocol DTOs into provider-neutral application contracts.
Existing CG services own semantics. No provider SDK, MCP framing or credential
types enter inner crates. EPIC-07 separately owns outbound connectors; only outer
framing utilities may be shared. Existing Cargo allowlists remain authoritative,
with explicit provider/MCP dependency mutation coverage.

## Consequences

- This architecture decision precedes runtime implementation. #237–#245 must
  implement and qualify protocol, admission, secret isolation and failure behavior.
- Private stdio trusts launch/OS integrity; it cannot prove a parent executable's
  vendor. Same-user/OS compromise is outside the isolation guarantee.
- No-key describes this CG connection, not Codex authentication or other providers.
- New network transports require reviewed trust/compatibility contracts. No
  automatic scope, version or authentication fallback exists.
- Crate guards enforce dependency edges. Locally defined provider-shaped DTOs
  still need module/type review; credential isolation needs runtime tests.

## Alternatives considered

- Direct provider API/SDK integration: rejected for this path because it requires
  CG provider credentials and introduces the wrong integration direction.
- Trust any local client or self-declared name: rejected because locality and
  discovery establish neither identity nor permission.
- Expose the outbound connector runtime directly: rejected because inbound
  admission and external capability/evidence lifecycles have separate owners.
