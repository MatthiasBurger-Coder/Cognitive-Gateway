# Versioned Codex-facing contracts

**Defined — EPIC-04.02 #237.** This is a provider-independent wire contract for
an outer driving adapter. [Server lifecycle/discovery](local-mcp-server.md)
(#238) and the [application facade](codex-application-facade.md) (#239) are
implemented. [Workspace/session admission](codex-scope-isolation.md) (#240) provides admitted situation queries and scoped resources; live runtime qualification remains #245.
The [local trust contract](codex-local-integration.md), ADR-020 and ADR-016 apply.
Rust domain/application types remain authoritative. JSON Schema validation is
necessary but does not prove domain validity, scope admission or authorization.

## Artifacts and ownership

[Version 1.0](../schemas/codex/v1/) contains Draft 2020-12 request, response,
resource, common and catalog schemas, plus the machine-readable
[tool/resource catalog](../schemas/codex/v1/catalog.json).
Schema `$id` values are identifiers resolved from local artifacts, not network
endpoints. Discovery must supply complete schemas with locally bundled `$defs`
and rewritten local references; clients must not need network schema resolution.

| Operation / MCP tool | Canonical owner / mapping | Result contract |
| --- | --- | --- |
| `situation.inspect` / `cg_situation_inspect_v1` | `DeclarativeSituationApplication::inspect_situation`; validated `DeclarativeContextSituationDocument` and optional pinned process snapshot | `cg.situation` |
| `situation.assess` / `cg_situation_assess_v1` | `DeclarativeSituationApplication::assess_situation`; validated `SituationAssemblyInput`, normalization and document assembly as in the existing `cg assess` path | `cg.assessment` |
| `state.explain` / `cg_state_explain_v1` | `DeclarativeResolutionApplication::explain_resolution`; existing `ResolvedPlan` | `cg.resolution-trace` |
| `capabilities.resolve` / `cg_capabilities_resolve_v1` | `DeclarativeResolutionApplication::resolve_plan`; `ResolutionSnapshotPort`, `CompositionRules`, existing pinned Process/Agent/Skill capability resolution | `cg.resolution` |
| `context.compile` / `cg_context_compile_v1` | `ContextApplication::compile_step`; `CompileStepInput`, current policy and host-authenticated disclosure policy | `cg.execution-context` |
| `registry.inspect` / `cg_registry_inspect_v1` | Read-only facade projection over `Registry`, `CapabilityIndex`, `ProcessRegistry`; existing integrity validation | `cg.registry` |
| `evidence.inspect` / `cg_evidence_inspect_v1` | Existing scoped evidence/provenance inspection from situation, resolution and compiled context; unavailable references fail closed | `cg.evidence` |

The request schema defines a strict input object for each operation. Each
reference includes an opaque ID, canonical contract discriminator, contract
version, immutable revision and SHA-256 digest. The facade resolves it against
admitted CG records, validates digest/revision and parses the authoritative Rust
contract. No client filesystem paths, URLs to fetch, credentials, policy grants,
raw process transitions or caller-authored authorization objects are accepted.

`cg.*` discriminators select existing payload parsers/projections; they do not
create replacement domain types. `contract_version` is the version of that
payload, independent of envelope `schema_version` and immutable record revision.
For example, a `cg.resolution` document retains its existing numeric
`schema_version: 1`; its boundary discriminator version is `"1.0"`.
`cg.situation` input selects the existing situation/document parser;
`cg.situation-assembly` selects the existing assessment assembly input mapper. `cg.assessment` retains the current `cg assess` output
wrapper, including its authoritative `document` and explanation. Registry and
evidence projections contain only existing records, in canonical identity order;
there is no new evidence repository. Unsupported payload versions fail closed.
The facade must register exact supported payload parsers before exposing a tool.

Input `kind: document` wraps a canonical document; `kind: reference` wraps a
pinned reference. The opaque `document` object is deliberately validated by its
Rust owner, with that owner's unknown-field/version rules. It is not a place to
accept arbitrary adapter extensions or authorization assertions. Schema-valid
but domain-invalid input yields `CG_INVALID_INPUT` before execution.

## Envelopes and admission

Requests require `schema_version`, `scope`, `operation`, `input`, `execution` and
`correlation`. Scope names workspace, project and **local binding**. `binding_id`
is the trusted local admission binding from #240, distinct from a task SessionId.
No scope is inferred from cwd or a previous request. IDs are opaque ASCII tokens;
references and resource URIs must match the admitted scope before existence or
content is disclosed. `execution` uses the existing OperatingMode/ExecutionProfile
wire values and conveys requested depth, never elevated authority. Request/trace
IDs correlate calls and do not provide idempotency or authority.

Responses require version, scope, operation, correlation, status, result,
explainability references, evidence references, provenance and diagnostics.
`ok` carries exactly one query or session result and no failure diagnostic.
A query's `canonical_result` is an existing document or pinned reference, never
an execution permission. Provenance preserves source identity/revision, digest,
freshness, sensitivity and lineage. Unclassified source material fails closed;
empty provenance/evidence arrays mean none exists, not successful verification.
`SECRET` is reference-only and requires authorized metadata filtering; no raw
secret may appear in documents, provenance, diagnostics or text fallbacks.

Non-`ok` responses have a null result and exactly one diagnostic. They contain
no protected evidence, provenance or explanation references. Scope, operation
and correlation may be null when admission cannot safely establish them; a
rejected input is never copied into these fields. Failure responses use version
1.0 as the server diagnostic contract, not as a negotiated downgrade.

## Session projections (#272)

| Operation | Input addition | Classification | Current availability |
| --- | --- | --- | --- |
| `session.start` | Command ID, existing intent document/reference | Mutate | Unsupported |
| `session.inspect` | SessionId | Inspect | Unsupported |
| `session.clarify` | SessionId, command ID, expected revision, pending ID, answer | Mutate | Unsupported |
| `session.approve` | SessionId, command ID, expected revision, pending ID, pinned **trusted CG consent record** | Mutate | Unsupported |
| `session.continue` | SessionId, command ID, expected revision | Mutate | Unsupported |
| `session.cancel` | SessionId, command ID, expected revision | Mutate | Unsupported |

Tool names follow `cg_session_<command>_v1`. These commands must delegate to
#272/#273's shared application API. The catalog reserves their contracts and
marks them unsupported; it does not advertise executable session support.
Invocations return `unsupported` / `CG_UNSUPPORTED_CAPABILITY`, not a simulated
session or an automatic one-shot fallback. `defined` on other catalog entries
means a contract/service mapping exists, not that an MCP server is running.

Future session responses project SessionId, revision, lifecycle status, pending
clarification/consent references and a verified final-result reference. Only
`completed` has a non-null verified final result; pending states require matching
pending entries and cannot carry final results. Running, failed and cancelled
states have neither pending entries nor a verified final result. Statuses are
wire projections of the shared host lifecycle, not a second process state machine.
An unverified proposal cannot be labeled a completed result.

Session commands must validate expected revision, command uniqueness, current
scope and authority. Stale, duplicate, cross-scope and invalid/terminal-state
commands are refused; terminal sessions cannot silently restart. A client answer
or consent-record reference cannot grant permission. Approval validates a trusted
CG consent record against current policy, exact scope and pinned revisions.
Run/dispatch IDs and richer lifecycle fields belong to the shared #272 contract;
adding them requires an explicitly negotiated boundary revision.

Transport cancellation/disconnect ends the call, not the task session.
Only authorized `session.cancel` asks the shared API to cancel the session.
Reconnect/resume revalidates admission, scope, current policy and revision-bound
consent. If a mutating call may have committed before disconnect/timeout,
`CG_OUTCOME_UNKNOWN` requires session inspection; never automatically replay it.
MCP task-augmented execution is forbidden in v1; CG SessionId is not an MCP task ID.

## MCP projection and resources

The selected local transport is private stdio; the initial protocol allowlist is
`2025-11-25`, independently negotiated through MCP initialization. The normative
projection follows the official [tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools),
[resources specification](https://modelcontextprotocol.io/specification/2025-11-25/server/resources)
and [lifecycle specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle).
For `tools/call`, `arguments` is the complete request envelope. The MCP tool name
must map to exactly the envelope operation; mismatches yield `CG_INVALID_REQUEST`.
Discovery publishes an operation-constrained request `inputSchema` and response
`outputSchema`. Catalog schema filenames are artifact references, not literal
MCP `inputSchema` values. Bundle referenced schemas before discovery.

Tool results place the response envelope in `structuredContent` and its identical
serialized JSON in one text block. `isError` is false only for `status: ok`.
Application failures are tool results, not JSON-RPC errors. Malformed JSON/framing
and unknown MCP methods use JSON-RPC -32700/-32600/-32601 respectively; invalid
MCP parameters/unknown tool names use -32602, with fixed sanitized messages.
No invalid frame is echoed. Schema/version/admission failures in a decodable
recognized tool call use the versioned failure envelope without application dispatch.

| Resource URI/template | Content | Authority |
| --- | --- | --- |
| `cg://contracts/1.0/catalog` | Static catalog | Discovery only |
| `cg://contracts/1.0/{schema}` | Allowlisted schema artifact (`common`, `request`, `response`, `resource`, `catalog`, each with `.schema.json`) | No filesystem/template traversal |
| `cg://workspaces/{workspace_id}/projects/{project_id}/bindings/{binding_id}/references/{id}/{revision}/{digest}` | `resource.schema.json` envelope with existing canonical document and provenance | Same CG scope/policy/disclosure checks as tool calls |

All resources use `application/json`; UTF-8 JSON is carried in MCP resource
`text`. URI components must be percent-encoded and parsed once; malformed,
ambiguous, unknown or cross-scope references are rejected. Resource reads cannot
mutate state or trigger new external fetches. A document is returned only when
its canonical contract/version/revision/digest and disclosure policy agree.
Pinned resources are immutable; a changed source creates a new reference.
Only static contracts need be listed; scoped references can be tool-returned.
No subscriptions or notifications are promised by v1. Resource errors return
sanitized JSON-RPC -32001 with the fixed diagnostic object in `error.data`;
resource reads never return a success-shaped error document.

## Side effects and diagnostics

Inspect tools have `readOnlyHint: true`, `destructiveHint: false`,
`idempotentHint: true`, `openWorldHint: false`. Their only allowed operational side
effect is bounded sanitized auditing; they do not mutate domain state or call
external connectors. Session commands are Mutate, conservatively set destructive
true and idempotent false, and remain disabled. These annotations describe the
contract and grant no CG permission. Runtime classification must use the complete
canonical capability closure. A read operation cannot acquire mutation effects.

Diagnostic code/message/retry triples are frozen in `common.schema.json`.
No free-form exception messages, rejected values, paths, secrets or cross-scope
existence details enter diagnostics. The first applicable failure is selected in
this order: bounded decoding, version, operation/schema, binding/scope, canonical
input/reference validation, current policy/disclosure, process/session gate,
execution. Choose one code; never rely on unordered exception iteration.

| Codes | Response status / meaning |
| --- | --- |
| `CG_INVALID_REQUEST`, `CG_UNKNOWN_OPERATION`, `CG_INVALID_INPUT` | Error; shape, operation or domain validation failure |
| `CG_UNSUPPORTED_VERSION`, `CG_UNSUPPORTED_CAPABILITY` | Unsupported; exact version/capability unavailable |
| `CG_SCOPE_DENIED`, `CG_POLICY_DENIED`, `CG_SENSITIVITY_DENIED` | Denied; no existence disclosure or protected output |
| `CG_REFERENCE_UNAVAILABLE`, `CG_STALE_REVISION` | Error; unavailable authorized reference or failed revision precondition |
| `CG_CONSENT_REQUIRED`, `CG_EVIDENCE_REQUIRED`, `CG_PROCESS_BLOCKED` | Blocked; existing CG prerequisites, no execution |
| `CG_DUPLICATE_COMMAND`, `CG_INVALID_SESSION_STATE` | Error; shared session API refusal |
| `CG_CANCELLED`, `CG_TIMEOUT`, `CG_LIMIT_EXCEEDED` | Error; bounded call failure |
| `CG_OUTCOME_UNKNOWN` | Error; inspect command outcome before any next action |
| `CG_INTERNAL_ERROR` | Error; sanitized fallback, never raw exception text |

Map domain validation to `CG_INVALID_INPUT`, snapshot/revision mismatch to
`CG_STALE_REVISION`, missing authorized reference to `CG_REFERENCE_UNAVAILABLE`,
policy Deny/RequireConsent/RequireEvidence to the corresponding CG code, process
blockers to `CG_PROCESS_BLOCKED`, and unmapped implementation errors to
`CG_INTERNAL_ERROR`. Do not reinterpret the core's decision. Detailed domain
reason codes remain in authorized canonical explanation documents, not leaked
in failure messages. `retry` is `never`, `after_change` or `inspect`; it is advice,
never permission to replay a mutation or skip reauthorization.

## Negotiation and compatibility matrix

| Client / server condition | Required behavior |
| --- | --- |
| MCP `2025-11-25`, envelope `1.0`, supported payload versions, admitted client | Compatible subject to scope and policy |
| Other MCP protocol date | No application access; initialization selects only the allowlist; incompatible client disconnects |
| Missing/unknown envelope version, including `1.1` or `2.0` | `CG_UNSUPPORTED_VERSION`; no guessed downgrade |
| Unknown field in envelope or adapter-owned nested object | `CG_INVALID_REQUEST`; strict `additionalProperties: false` |
| Unknown field/version inside canonical document | Existing Rust parser policy; invalid input or unsupported version; never permissive adapter parsing |
| Known operation with unavailable owner | `CG_UNSUPPORTED_CAPABILITY`; no fallback or fabricated result |
| Unknown operation | `CG_UNKNOWN_OPERATION`; no dispatch |
| New optional field, enum, diagnostic or operation | New immutable contract revision and explicit allowlist negotiation |
| Renamed field/operation or changed semantics | New major contract; retain old published artifacts while supported |

Clients inspect the static catalog after MCP initialization; trusted launch
configuration must additionally admit the client and exact envelope version.
Every call states its exact envelope version. There is no range negotiation,
implicit extension map or automatic minor-version compatibility in v1. Payload
versions evolve independently through exact parser registration. Operational
correlation fields do not enter canonical domain identity/cache keys.

## Serialization and reproducible evidence

JSON is UTF-8, without duplicate keys, non-finite numbers or insignificant
transport-dependent values in canonical payloads. IDs are case-sensitive.
Adapter envelopes use lexical object-key order and compact separators for
canonical comparison; pretty fixture whitespace is presentation only. Integers
in adapter-owned fields are restricted to the interoperable safe range. Domain
serializers retain their existing numeric/digest rules; the adapter must not
re-hash a pretty-printed payload as a new domain identity.

Set-like envelope collections (`ids`, references, provenance and pending entries)
are unique and ordered lexically by canonical ID, then contract, contract version,
revision and digest as tie breakers. Provenance sorts by source ID/revision, then
reference; diagnostics follow the fixed selection rule. JSON Schema enforces
uniqueness, while the facade enforces ordering. Preserve sequence-sensitive
arrays inside canonical documents, including plans, trace paths and process
history. No blanket array sorting or Unicode/number rewriting is permitted.

Equivalent admitted canonical inputs, pinned registry/process/policy/source
snapshots and execution settings must produce equivalent canonical results,
explanation/evidence/provenance and diagnostics. Correlation IDs, timing and
audit events are excluded. Different authority/source revisions are different
inputs. This slice defines and tests wire invariants; runtime determinism and
security proof remain #239/#245 responsibilities.

[Frozen examples](../tests/fixtures/codex-v1/) cover every request/tool result,
every diagnostic, all planned session projections and a pinned resource. The
assessment payload is copied exactly from the existing CG-12 golden output;
a Rust contract test parses/serializes it through the authoritative owner.
Other all-zero reference digests are explicitly symbolic unresolved examples,
not verified artifacts or admission tokens. `projection.*` files describe future
wire shapes; actual session pairs return unsupported.

```sh
python3 -m pip install -r tests/contracts/requirements.txt
python3 -m unittest discover -s tests/contracts -v
cargo test -p gateway-daemon --test codex_contracts --locked
cargo test -p gateway-daemon --test declarative_cli --locked cg12::
bash scripts/check-architecture.sh
python3 -m unittest discover -s tests/architecture
cargo fmt --check
```

The Python suite is an offline schema/fixture validator, never runtime or domain
authority. It validates all schemas/examples and rejects incompatible inputs,
authority flags, mismatched input/result contracts, sensitive diagnostic text and
invalid session projections. Rust tests pin the wire execution enums to existing
domain enums and preserve the canonical situation payload. These run in the
existing quality gate without weakening coverage. Run the full
`python3 scripts/quality-gate.py` for release qualification; focused checks do
not claim a working MCP endpoint or completed session E2E. No executable behavior
is added in #237, so live MCP E2E is deferred to the implementation slices.

| #237 criterion | Evidence |
| --- | --- |
| Provider independence / existing authoritative types | Schema DTO boundary, service mapping, Rust `codex_contracts` and architecture dependency checks |
| Deterministic canonical output | CG-12 unchanged golden payload, domain serialization roundtrip, ordering rules; future facade runtime proof explicitly deferred |
| Unknown fields/versions | Strict schemas, compatibility matrix, negative contract tests |
| Inspect/Mutate distinction | Catalog classification/annotations, session unsupported examples and catalog tests |
| Stable non-sensitive errors | Frozen diagnostic triples, failure schema restrictions and negative tests |
| Reproducible fixtures | Frozen examples, offline schema tests, Rust owner tests, mandatory quality-gate entry |
| One-shot/session/pending/verified result distinction | Tagged query/session schemas and positive/negative lifecycle projection tests |
| #272 projections without new authority | Reserved shared command mappings, revision/command fields, explicit unsupported catalog/results |
