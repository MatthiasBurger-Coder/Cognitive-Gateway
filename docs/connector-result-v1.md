# Canonical connector result and provenance boundary

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

EPIC-07.05 #228 implements `gateway_application::connector_results::ConnectorResult`
and the outer `mcp_client::normalization::CanonicalResultNormalizer`. The Rust
contract contains no provider JSON. The [normalization fixtures](../tests/fixtures/connector-result-v1/cases.json)
and [evidence record](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-07.05-normalization.json) qualify this slice.

The application port revision is now **2** because `ConnectorObservation` requires
`ConnectorResultMetadata`. Existing methods and operation classes are unchanged.
Manifest format remains 1.0; `compatibility.port_version` must be `"2"`. Rebuild
adapters, supply the required metadata, regenerate manifest digests and rediscover
inventories. Old port-1 manifests fail closed. Fixture manifests and independently
hashed inventory vectors are migrated together; historical evidence keeps its
original revision. There is no persisted application-result format migration.

## Acquisition and projection

The existing immutable discovery mapper validates the complete inventory, dispatch
bindings and closed scalar result schema. Its mandatory normalizer now has a
concrete implementation. `NormalizationProfile` is CG configuration: an exact
canonical source, source kind, provider object field, optional revision/digest
fields and explicit scalar-field-to-`SubjectPath` projections. Missing/conflicting
source mappings, capability/class substitution, unsupported values, empty strings,
missing fields and malformed revisions/digests fail closed. Provider annotations
and text instructions cannot define projections, quality, scope or authority.

Successful tools require validated structured content. Auxiliary MCP content is
not projected. The supported resource profile requires one text record; its URI
is the object reference and its text remains a string even when it resembles JSON.
The host validates the exact requested URI. Blob/multipart resources and nested,
array or decimal payload projections fail closed pending separately qualified
profiles. Configurations have at most 256 distinct subjects. Encoded projected
input is capped at 256 KiB; the protocol host additionally bounds wire input.

`AcquisitionPort` is a required trusted host dependency. It receives the admitted
dispatch, never provider JSON, and supplies authenticated collector identity,
acquisition/observation/evaluation time, maximum age, handling sensitivity,
conflict/partial state and observation/effect basis. No permissive authentication
or clock default is provided. #229 supplies production credential authentication;
the port's identity is an audit reference, not cryptographic proof of a grant.
Provider claims cannot upgrade trust or downgrade sensitivity. The profile uses
existing `TrustClass::ObservedEvidence`; model explanations are rejected as a
source kind and remain on the separate model-invocation plane.

## Canonical envelope and reuse

`ConnectorObservation` binds target/scope, inventory digest, correlation, canonical
source, optional opaque source revision, computed content digest, acquisition time,
validated `ObservationEvidenceSet`, basis and metadata. Metadata retains task,
step, execution, object identity, source kind, optional provider digest, observation
time, authenticated source, existing `QualityMetadata`, effect uncertainty, partial
state, optional original effect request and optional non-authoritative raw archive
reference. Provider revision is never synthesized from local acquisition time.
The provider digest is retained separately from the SHA-256 computed over compact,
lexically keyed serde JSON input; the provider claim does not establish integrity.

Each projected scalar creates existing `Observation`, `Fact`, `Evidence` and
`Provenance` records with explicit support links. Values report what the source
said; they do not establish policy or completion. Object identity occupies
`Provenance.source_reference`, collector identity occupies `producer`, and source
revision/acquisition time retain distinct existing fields. Acquisition timestamps
in provenance are decimal Unix seconds. IDs use SHA-256 over versioned compact
JSON containing target, scope, capability/class, inventory, typed arguments,
task/step/execution/correlation, object/source, projection map, acquisition time and
content digest. Map ordering is lexical; output ordering uses existing CG-06
constructors. Identical replay inputs give identical records. Fresh acquisitions
have distinct lineage even when their content is unchanged.

`ConnectorResult::new` revalidates the public observation against dispatch,
source lineage, timestamps, trust, handling consistency and basis. The MCP host
calls the same validation after every normalizer, including custom adapters.
It bounds each graph collection to 256, checks all existing CG-06 relationships
and additionally rejects cyclic provenance. Existing parent chains (including earlier collectors and source objects),
source revision, evidence relationships and quality survive the adapters. The
existing CG-06 JSON serializer supplies lossless provenance round trips; this
slice creates no second evidence/provenance serialization vocabulary.

## Freshness, conflict and context

Freshness uses existing `evaluate_freshness`: explicit observed time and evaluation
time plus a finite maximum age. Unknown observation time remains UNKNOWN; receipt
time alone cannot make old content fresh. Future observations/acquisitions and
acquisition beyond the dispatch deadline fail closed. Stale and conflicting data
remain representable with unchanged quality and provenance. Partial results carry
INCOMPLETE uncertainty. Nothing silently overwrites or resolves competing evidence.
Stable content-free diagnostics distinguish stale, conflict, partial,
acknowledgement-only and uncertain-effect conditions; malformed inputs use fixed
`Error`/`ResultError` variants without retaining hostile messages.

`batch()` creates existing `SourceSnapshot` and `ScopedObservationBatch`, attaching
the unchanged quality to every subject for `ObservationEvidenceInputPort`.
`fragments()` uses the existing `ContextFragment::evidence` adapter, retaining
scope, step, source revision, provenance/evidence references and quality. Fragments
are references, not full payloads; selection, disclosure, retrieval and CG-10
minimization remain required. The optional raw archive reference is excluded from
both batch and context and cannot supply facts, instructions or authority.

## Acknowledgements and effects

Provider acknowledgement, source read and independent post-effect read remain
separate existing enum values. Acknowledgements retain correlation, partial state
and `ConnectorEffect`, including UNKNOWN, but `batch()` and `fragments()` reject
them. Ordinary source reads require an inspect/read/search dispatch and NoMutation.
An independent read-back also requires a linked original mutation/admin request
in the same target/scope and task, with different correlation and execution IDs.
The collector must establish independent acquisition; a provider enum cannot do
so. The original request makes step/dispatch correlation available to #274/#276.
The result does not assert that the observed object satisfies the expected goal.
CG-14/EPIC-08 own completion evaluation, session persistence and reconciliation.

After a possible mutation send, normalization/schema failure retains UNKNOWN via
the existing host failure path. An acknowledgement never upgrades it to verified
completion, and normalization never schedules retries or grants permissions.

## Review and verification

Intake: clean `4bdc95b7a559240cf19f4508dbd2c0d53c08b7e8`, DEVELOPMENT/FULL_PATH.
Requirement, architecture, automation and tester passes were performed by the
implementing agent; no independent approval is claimed. The slice was ready using
#225–#227 and existing CG-06/EPIC-02 types. Scope includes the v2 port migration,
normalization, adapters, fixtures, gates and evidence. No database/deployment change
is needed. Rollback restores the port-1 adapters/manifests and rediscovery vectors
together. Estimated scope was 1–2 engineer-days implementation and 0.5–1 day
qualification, assuming the existing host; provider deployment is outside scope.

| ID | Requirement | Production owner/path | Verification level |
| --- | --- | --- | --- |
| N01 | Validate provider outputs | mapper schema → CanonicalResultNormalizer → ConnectorResult → host | Malformed/authority-claim fixtures and host denial tests |
| N02 | Preserve identity/revision/digest/time | Existing Provenance and result metadata | Deterministic projection and serialization round trips |
| N03 | Preserve trust/sensitivity/freshness/conflict | AcquisitionPort → QualityMetadata → batch/fragments | Stale/conflict/partial and unknown-time tests |
| N04 | Reuse existing evidence and context | ObservationEvidenceSet, SourceSnapshot, ScopedObservationBatch, ContextFragment | Real stdio read → normalization → scoped store → context references |
| N05 | Raw data remains non-authoritative | Explicit archive metadata, excluded by adapters | Batch/context exclusion assertions |
| N06 | Acknowledgement/read-back and uncertainty | Basis validation, original request link, host failure effect | Ack denial, independent read-back/tampering and uncertain-mutation tests |

Run the added connector-result coverage gate (>=95% for each production file),
workspace tests/build/clippy/format, architecture/dependency checks and contract
fixtures. Real subprocess tests use the independent synthetic MCP peer and an
explicit test acquisition authority. They establish executable protocol-to-context
integration, not authenticated production-provider availability or real external
mutation completion. Credentials (#229), current production policy/consent (#230),
reconciliation (#231), installed coordinator wiring and parent live qualification
remain separate work packages.
