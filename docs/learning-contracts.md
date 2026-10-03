# CG-21: experience, pattern and learned procedure contracts

Issue: [#212](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/212). Rust contracts: [`gateway-domain::memory`](../crates/gateway-domain/src/memory.rs) and [`gateway-domain::learning`](../crates/gateway-domain/src/learning.rs). JSON Schemas: [`experience.schema.json`](../schemas/experience.schema.json) and [`learning.schema.json`](../schemas/learning.schema.json).

## Authority boundary

An `Observation` reports a source assertion. `Evidence` supports or challenges an assertion. A governed `ExperienceRecord` is a derived historical record with source snapshot, provenance, validation, outcome and label basis. A knowledge record is retrieved context. None grants permission. `PatternCandidate` is an inspectable hypothesis over validated experience; it contains no steps. `LearnedProcedure` declares possible steps and preconditions, but its status is not an execution token. Execution still requires current process registry resolution, policy evaluation, capability availability, current observations and evidence, and the existing process gates. Model output can propose a candidate; it cannot promote or execute a procedure.

`ExperienceBasis` pins the existing `MemoryEligibilityReference` plus provenance and an evaluation reference. The source memory reference must have schema version 1 and positive revision and eligibility version. A consumer must revalidate it at use time through the memory application; the domain contract cannot attest to current freshness or revocation. The fingerprint and every basis must use the same project scope. `SituationFingerprint` uses canonical typed `OperatingMode`, `CapabilityId` and `FactId` signals. It requires at least one fact or capability signal and cannot consist solely of free text or similarity scores.

`ExperienceRecord::to_json/from_json` uses a strict v1 wire with explicit optional fields and tagged inline/reference payload. It validates the existing time, trust and payload invariants on both directions. Curation and eligibility remain separate application decisions; a JSON round trip does not validate the source or authorize learning.

## Procedure content

One immutable procedure version contains an ID, positive version, source candidate ID, fingerprint, sorted experience bases, ordered steps, sorted required observation IDs, sorted required evidence IDs, sorted verification evidence IDs and fallback (`STOP` or `RETURN_TO_PLANNER`). Each step pins a process definition ID, positive version and lowercase SHA-256 digest, and references existing `CapabilityId` and `PolicyId` contracts. The domain crate deliberately holds a process reference with the registry's ID/version/digest shape because the process crate depends on the domain crate. Registry lookup, policy checks and process gate checks must happen when a procedure is considered for use.

The `LearnedProcedure` digest is lowercase SHA-256 of UTF-8 JSON serialization of the ordered `ProcedureContent` fields, excluding the digest itself. Field order is the Rust content declaration order; fingerprint signals, experience bases and required references are sorted, while steps retain declared order. Identity and version are included in the digest. A changed field requires a new version and digest. The digest detects content changes; it is not a signature, authorization or proof of evaluation. Writers must never replace a stored `(id, version)` with a different digest. The domain object is immutable through its public API.

The domain crate uses only the provider-independent `sha2` implementation for this deterministic verification. The repository dependency allowlist records that narrow addition; no adapter or runtime dependency enters the domain.

The v1 wire uses canonical enum strings, numeric versions, typed identifier strings and strict required fields. `from_json` checks schema version, ordering, uniqueness, cross-field invariants and procedure digest. Unknown fields fail. There is no implicit migration: readers reject unsupported versions and producers must create an explicit versioned migration in a later contract. The JSON Schema describes shapes; Rust validation additionally enforces ordering, scope consistency, process ID rules and digest integrity.

## Lifecycle

Lifecycle is a separate append-only decision projection for a fixed procedure ID and version. Every transition records source and target state, decision reference, actor provenance and explicit Unix time. `ProcedureLifecycle::apply` rejects a wrong identity or source state, repeated decision ID, reversed time or illegal edge. The legal edges are:

```text
DRAFT -> EVALUATED | REJECTED
EVALUATED -> APPROVED | REJECTED
APPROVED -> ACTIVE | RETIRED
ACTIVE -> SUSPENDED | RETIRED
SUSPENDED -> ACTIVE | RETIRED
```

`REJECTED` and `RETIRED` are terminal. A promotion decision is auditable history, not a substitute for runtime authorization. Changes to procedure content create a new immutable version and a new draft lifecycle.

## Examples

This is a valid candidate shape. Its memory basis must still pass current eligibility revalidation:

```json
{"schema_version":1,"id":"candidate-1","fingerprint":{"scope":"project-1","signals":[{"kind":"FACT","value":"fact-1"}]},"experience":[{"memory":{"schema_version":1,"scope":"project-1","id":"memory-1","revision":2,"eligibility_version":2,"source_snapshot":"snapshot-1","source_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"provenance":"source-1","evaluation":"evaluation-1"}]}
```

Invalid examples: `"schema_version":2`; a fingerprint containing only `OPERATING_MODE`; duplicate experience IDs; a source in another scope; process version zero; a changed step with an old digest; and `DRAFT -> ACTIVE`. Each fails before it can be treated as a usable procedure.

## Verification

`cargo test -p gateway-domain --test learning_contracts` covers canonical round trips, digest binding, unsupported versions, duplicate signals and lifecycle transitions. Domain contracts are provider independent and add no executable runtime path, so real end-to-end execution is outside this contract slice.
