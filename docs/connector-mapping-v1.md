# Deterministic connector discovery and mapping v1

EPIC-07.04 #227 supplies an explicit, provider-independent mapper for the
[admitted outbound MCP host](mcp-client-runtime.md). The implementation is
[`mcp_client::mapping`](../crates/gateway-daemon/src/mcp_client/mapping.rs);
the [mapping schema](../schemas/connector-mapping-v1.schema.json) and
[fixtures](../tests/fixtures/connector-mapping-v1/) pin its contract.
It introduces no dependencies or catalog writes. Discovery grants no authority.

`MappingDocument::from_json` parses an operator-owned mapping document, never
server instructions. The document has `mapping_version: "1.0"` and at most 256
records. Input is capped at 262144 bytes with bounded JSON nesting; duplicate
JSON keys, unknown fields, versions, enums and invalid canonical IDs fail closed.
Records are sorted by reference. Optional `source`, `idempotency_key` and
`result_schema` fields normalize to null. Other metadata is required.

Each record binds one immutable versioned `reference`, manifest `kind` and `name`,
canonical `CapabilityId`, optional `SourceId`, operation class, side effects,
idempotency, retry safety, consent hint and result contract. References use the
manifest reference alphabet. Reference names should include their release, as
in `mapping.records.read.v1`; reference spelling alone does not prove immutability.
The document is immutable after construction and its full content digest binds
every inventory. Replacing any record requires rebuilding discovery and fresh
application admission; a changed record under the same reference cannot reuse
old inventory revisions. Retain old documents for replay.

| Class | Existing CG class | Required side effects |
| --- | --- | --- |
| READ, SEARCH, INSPECT | `CapabilityClass::Inspect` | `none` |
| MUTATE | `CapabilityClass::Mutate` | `mutation` |
| ADMIN | `CapabilityClass::Mutate` | `administrative` |

`MappingRecord::capability_definition()` returns the existing CG-03 definition
and ID for explicit registry/resolution consumers. The derived inventory uses
existing `ConnectorInventory`, `ConnectorCapability`, `CapabilityId` and `SourceId`.
It never creates Agent/Skill/Process membership or changes a registry. ADMIN
retains its distinct operation class and requires separate policy evaluation.

Idempotency is `non_idempotent`, `idempotent` or `provider_key`. A provider key
must name a required string argument in the discovered input contract. Retry
safety is `never`, `read_only` or `idempotent`: mutation/admin records cannot use
`read_only`, and `idempotent` requires explicit idempotency support. Consent is
`policy` or `required`. These records preserve configured semantics for policy
and resilience consumers; they cannot establish a consent receipt, authorize an
effect, schedule a retry, resolve uncertainty or replenish a budget. Remote
annotations never supply or override these values, even `readOnlyHint: true`.

## Complete discovery and contract validation

`DiscoveryMapper` binds an exact target/scope, validated #225 manifest and mapping
document. `snapshot` accepts the complete bounded `ProtocolInventory` collected
by #226 after the pinned MCP 2025-11-25 handshake. The runtime already rejects
partial pagination and unsupported negotiation. The pure mapper cannot establish
that a standalone caller actually negotiated a connection; host composition owns
that precondition. It also caps aggregate items at 1024 and encoded bytes at 4 MiB.

Every tool, resource and prompt must match exactly one manifest `(kind, name)`.
Resources additionally require unique, nonempty exact URIs; duplicate names or
URIs fail. All declared entries must be present. Unknown entries, missing entries,
duplicate wire identities, unresolved/unused mapping records and conflicting
canonical IDs reject the entire snapshot. No partial executable index is returned.

Tools resolve `capability_mapping_ref`; resources resolve `source_mapping_ref`.
If both references are present, both must identify the same combined record.
Presence of a source reference must agree with the record's canonical source.
Resources require an explicit READ/INSPECT capability and source, with no arguments
or template expansion. Tools may carry source semantics but execute only through
`invoke`; resource dispatch cannot be substituted for a tool call.

The manifest's entry `schema_digest` is SHA-256 of compact, recursively lexical
object-key JSON for:

```json
{"discovery_schema_version":"1.0","entry":{"name":"example","inputSchema":{}},"kind":"tool"}
```

`entry` is the **complete actual wire entry**, including schemas, annotations,
description, URI and MIME metadata where present. The example shows the encoding
shape; its empty schema is intentionally not an executable contract. Pinning the
whole entry conservatively invalidates changes to descriptive metadata as well.
The Rust helper `discovery_schema_digest` and independently encoded Python golden
vectors implement the same encoding. Discovery list order and JSON object-key
order do not affect revisions; order inside schema arrays is preserved.

Tool inputs and outputs use the qualified closed object schema profile: named
string, signed/unsigned integer and boolean properties, required-property lists,
`additionalProperties: false`, optional string title/description and optional
exact draft 2020-12 dialect. Input integers must also fit the existing typed CG
argument representation. Unknown keywords, duplicate or undeclared required
properties, malformed metadata, open objects, references, nested values,
arrays, decimal constraints and other dialects fail closed, including unsupported
optional properties before any invocation. No approximation or model inference
is performed.

Tools require an explicit Gateway result schema. An advertised `outputSchema`
must equal it. When the server omits `outputSchema`, the adapter still checks
successful `structuredContent` against the configured contract before calling
the normalizer. Tool errors retain `is_error` for the required normalizer and do
not become successful results. Resources use the existing runtime-validated
text-or-blob envelope with the exact requested URI. Prompts and resource templates
remain metadata only; there is no execution or URI-template expansion route.
Templates have unique nonempty URI templates and names and participate in the
revision even though manifest v1 has no template mapping kind.

## Canonical inventory and dispatch

The snapshot stores entries sorted by canonical capability ID, sources sorted by
canonical source ID, and complete discovery sorted by canonical entry bytes.
Templates sort by `uriTemplate`. Its compact canonical audit JSON includes:

- `inventory_version`, connector and exact scope;
- manifest digest and the host-pinned negotiated protocol;
- full mapping document digest;
- complete wire inventory, including prompts/templates;
- resolved mapping records, exact wire identities and input/result schema digests.

`ConnectorInventory.revision` is lowercase SHA-256 over those exact audit bytes.
This is CG Mapping Canonical JSON v1, using the locked Rust `serde_json` Value
encoding with lexical object keys; it is not RFC 8785. Strings are UTF-8 and
array order is retained except for the explicitly sorted inventory lists. No
semantic equivalence is inferred for schema arrays or numeric spellings in
untrusted descriptive metadata. There are no clocks, generated IDs or session
counters. `MappedInventory` exposes
read-only entries, the existing application inventory and canonical audit bytes.
These are trusted inspection data, not automatically model-visible context.
The revision binds all operation metadata, so changes to class, consent, retry
safety, schema, source, scope, manifest or descriptive discovery invalidate it.

`resolve` rebuilds the snapshot and checks the request's exact target, revision,
capability, class, source and typed required arguments. It returns only the bound
tool name with unchanged arguments or the bound exact resource URI. Resource
arguments, unknown properties, unsupported typed values and source substitution
fail before transport. `CanonicalConnectorMapping` implements the existing host
mapping trait using this resolver and a mandatory `ConnectorResultNormalizer`.
There is no default/pass-through normalizer. The host's separate trusted
`ConnectorAdmissionPort` must still admit discovery, authorize each proposal and
revalidate immediately before effects. #228 owns canonical normalization; #230
owns production policy/consent services; #231 owns retry/reconciliation decisions.

Failures are fixed `MappingError` variants: encoding/version/metadata, duplicate
or conflicting mappings, target/discovery, unknown/missing entries, unresolved
references, schema digest/contract, stale inventory or invalid request. They
retain no untrusted text or arguments. The host adapter projects schema failures
to `SchemaMismatch` and stale dispatch to `InventoryChanged`; callers can use the
pure mapper for precise inspection diagnostics.

## Review, traceability and qualification

Intake: clean revision `8b42ea42ca104661cd2622968dad8dd45633c434`, DEVELOPMENT,
FULL_PATH. The implementing agent performed requirement, architecture, automation
and tester/evidence role passes; no independent review/approval is claimed.
Readiness was READY_FOR_WORKFLOW for #227: #225/#226 contracts are implemented;
normalization and current policy remain mandatory extension ports. Scope is
mapping, schema checks, host adapter, fixtures, documentation and evidence.
Planning range: 1–2 engineer-days implementation plus 0.5–1 qualification,
assuming the delivered protocol host; no external provider deployment included.
Rollback removes the additive adapter/document and restores the prior schema
validator; no database, installation or canonical catalog migration is involved.

| ID | Requirement/source | Production path/owner | Dependencies and actual state | Verification/evidence level | Status |
| --- | --- | --- | --- | --- | --- |
| D01 | Same manifest/discovery yields same mapping | DiscoveryMapper, canonical hashing | #225 immutable manifest and explicit document | Rust golden/permutation tests, independent Python bytes/digests | VERIFIED at component boundary |
| D02 | Unknown/ambiguous mappings fail closed | Strict document parser and complete snapshot | #226 complete bounded protocol discovery | Duplicate/ref/conflict/missing/unknown/schema-denial fixtures and stdio unknown-tool denial | VERIFIED at runtime component boundary |
| D03 | Discovery never authorizes | CanonicalConnectorMapping → private admitted host | Trusted admission mandatory; full #230 service pending | Real stdio discovery followed by authorize/revalidate denials | VERIFIED at runtime component boundary |
| D04 | Explicit mutation semantics | MappingRecord invariant checks | Existing Inspect/Mutate classes; no new policy | All five class mappings, side-effect/retry contradiction tests | VERIFIED at component boundary |
| D05 | Preserve retry/idempotency | MappingRecord, snapshot/digest | #231 reconciliation separate | Provider-key required-input tests and metadata-sensitive revision tests | VERIFIED at component boundary |
| D06 | Existing capability/source semantics | Existing application inventory and CG-03 definition | CG registry remains Agent/Skill authority | Typed capability/source assertions, existing registry query resolution and exact resource dispatch | VERIFIED at component boundary |

Run the EPIC-07.04 command in `scripts/quality-gates.json`, independent contract
tests and architecture/format/clippy checks. The [retained evidence](evidence/EPIC-07.04-mapping.json)
binds results to candidate sources. Runtime tests use an independent stdio
subprocess with test-only admission/normalization; they do not qualify live
providers, production credentials/policy/normalization, installed coordinator
wiring or completion of parent EPIC-07.
