# Evidence and grounding verification

Issue #267 adds version 1 Rust grounding contracts and deterministic verification
to the [output verification pipeline](output-verification.md). The verifier reuses
EPIC-02 `KnowledgeProvenance`, CG-06 `ObservationEvidenceSet`, scoped revision pins
and the existing quality/freshness types. A supported model explanation and an
observed external effect have separate acceptance conditions.

## Integration and ownership

The production application path is `parse_output` → `verify_output` →
`GroundingEvidencePort` → `verify_grounding` → existing semantic and current
admission checks. `OutputVerificationPort::grounding` exposes the evidence-owner
adapter. Tasks requiring `EvidenceRequired` or `AllClaimsSupported` cannot pass
without this adapter, even when `evaluate` reports every semantic check Passed.
Other tasks can opt in by returning the adapter. No retrieval coordinator, policy
engine, process owner, provider runtime or external dispatcher is introduced.

The evidence adapter has two required operations:

- `manifest` applies the admitted task's deterministic rules to the complete
  output, including tool arguments, and projects every factual assertion and
  citation. Each claim binds a result JSON pointer and exact wire value to a
  typed subject/value, fact ID and exact evidence pins. Claims of successful
  actions additionally carry the host-issued action pin. Unsupported statements,
  ambiguous mappings and unprojectable assertions must return a rejection.
- `resolve` authenticates each citation against current scoped storage and returns
  its complete relevant support/challenge basis, independent source pin, EPIC-02
  source/revision, CG-06
  lineage, quality, observation time and optional independent effect observation.
  It checks actual record content against the requested digest/revision, scope,
  parent provenance and current policy. It must include uncited contradictions
  and prevent model-derived records from acquiring observational provenance.

Neither operation may trust model-declared claim inventories, source labels,
confidence, completion flags or critic opinions. These are trusted adapter
obligations: the core cannot infer factual meaning from arbitrary natural
language or authenticate a remote database using a supplied hash alone. There is
no permissive store implementation. Live store/provider composition remains with
the owning hosts; the shipped MCP launchers do not expose this new service yet.

## Deterministic checks

The manifest version must be 1.0 and match the invocation identity and digest of
the exact response bytes. Claims must be nonempty, have distinct existing result
pointers, match actual wire values and carry distinct well-formed citations.
The result schema, reference legality, complete semantic decisions and policy/
process constraints remain mandatory in the surrounding output verifier.

For every citation the core checks exact scope and pin equality, the independent
source pin and source revision,
CG-06 lineage validity, source-reference consistency and configured accepted trust.
The source pin must identify the CG-06 provenance source; its revision must match
EPIC-02 metadata. Evidence record and source revisions may differ.
Model/synthetic provenance and evidence kinds are rejected. Evidence sensitivity
cannot exceed the admitted invocation's sensitivity. Freshness requires both a
Fresh quality classification and a timestamp within the explicit maximum age;
missing, future or expired timestamps fail. The evaluation time is supplied by
the host rather than read from an ambient clock.

A fact must match the claim's subject and typed value and have an explicit Supports
edge from the cited evidence. Challenges, unresolved conflict/uncertainty and
opposing facts for that subject reject the result. A source's existence or a
citation's syntactic validity does not establish support.

For an external-effect claim, the observation must match the complete action pin,
including scope, revision and digest. The core additionally requires ObservedEvidence
trust, Tool/Runtime/CI provenance, a ToolOutput/TestResult/Measurement/Artifact
record and a linked observation with the same provenance, subject and value.
A retrieved report, model explanation, confidence 1.0 or `completed` flag cannot
establish an action's success. This checks an independently supplied observation;
CG-14 and Process/Policy retain task completion and execution authority.

## Diagnostics and operations

Grounding failures are retained as `GROUNDING_FAILED` with a stable nested
`GroundingError` in the existing invocation/output-bound `VerificationRecord`.
Missing integration is `VERIFICATION_UNAVAILABLE`; a mismatched manifest is
`CORRELATION_MISMATCH`. The full diagnostic inventory is frozen in
`tests/fixtures/grounding-v1/diagnostics.json`. Positive critics cannot override
rejections. Current admission is checked again after evidence validation, so
context changes or authority revocation during verification still reject.

Retain the trusted manifest, resolved evidence pins and evaluation time alongside
the verification/transport records under the original scope and disclosure policy.
The public record omits claim text and source content. Do not downgrade sensitivity
or export evidence content in diagnostics. On stale evidence, refresh through the
owning EPIC-02/CG-06 service and construct a fresh invocation where its inputs
changed. On unavailable storage, fail verification. Replay revalidates records;
it does not dispatch the recorded tool proposals. No new credentials, storage
schema or launch configuration is required. Rollback removes the additive grounding
adapter and service together; evidence-required tasks continue to fail closed.

## Requirement and delivery review

Inspected base: `4922d40b346d320760436a455e2f2452f0291229`, initially clean,
DEVELOPMENT / FULL_PATH. Sources: #267, dependency #266, parent #178 including its
2026-10-04 integration additions, ADR-007/008/015 and existing evidence/quality,
semantic-output and invocation authority contracts. Requirement, architecture,
automation and testing reviews were role passes by the implementing agent;
no independent review is claimed.

Readiness was READY_FOR_WORKFLOW for the contract/application-service scope.
#266 already supplied parsing, exact invocation binding and mandatory semantic
checks. EPIC-02/CG-06 types and freshness evaluation existed; the grounding
algorithm and evidence-owner adapter interface were included in this slice.
Ordered work: define trusted projection/snapshot contracts; implement support and
effect validation; integrate mandatory semantic checks; qualify adversarial
fixtures, measured coverage and architecture; retain evidence. Effort estimate:
1–2 engineer-days implementation and 0.5–1 qualification, moderate confidence,
including these foundations. Live deployment integration is downstream work.

| ID | Requirement/source | Production path and owner | Dependencies and state | Automated evidence | Status |
| --- | --- | --- | --- | --- | --- |
| G01 | #267 fabricated citations and scope | Domain manifest gate; application exact-pin/source checks | Existing pins and EPIC-02 provenance; current authentication belongs to store adapter | Invalid/duplicate citations, changed pin/source, missing revision, cross-scope pins | VERIFIED component with synthetic store |
| G02 | #267 stale, contradictory, unsupported claims | CG-06 lineage and quality/freshness checks | Existing facts, supports/challenges, explicit host clock and freshness policy | Expired/missing/future times, opposing facts/challenges, unknown/conflicted quality, absent support | VERIFIED component with synthetic store |
| G03 | #267 external actions need observations | Action pin and CG-06 observation matching | Independent observation supplied by trusted effect owner; no dispatch | Positive observed action; absent/wrong action, wrong observation, retrieved/model explanation and confidence rejection | VERIFIED component with synthetic observer |
| G04 | #267 trust and sensitivity | Allowed trust plus invocation sensitivity checks | Existing quality types; host checks parent lineage and disclosure | Model/synthetic sources and evidence, denied trust, confidential data rejection | VERIFIED component with synthetic store |
| G05 | #266/#178 mandatory deterministic verification | `verify_output` requires grounding for evidence/claim rules | Real admitted IR; existing Process/Policy/semantic checks remain mandatory | Host Passed without adapter rejected; adapter success accepted; unavailable and changed-context denial | VERIFIED application integration with synthetic host |
| G06 | #178 coverage, fixtures and operator guidance | Per-file coverage gate; quality/architecture configuration | Existing Rust/Python tooling | See [retained qualification](evidence/EPIC-06.07-grounding.json) | VERIFIED component; per-file coverage ≥95% |

Run the EPIC-06.07 entry in `scripts/quality-gates.json` to reproduce measured
coverage. The component tests use real canonical IR construction and real domain
records with synthetic authentication/storage/observation ports. They establish
the deterministic service contract; full installed-host/provider integration,
authentication of live external effects and parent EPIC-06 acceptance remain
separate qualification scopes.
