# Schema, reference and constraint verification — #266

`gateway-application::output_verification` verifies model proposals against the
actual admitted `InvocationIr`. It reuses `semantic_output::OutputContract`,
`VerificationContract`, `assess_result`, CG-10 task bindings and the captured tool
inventory. Version 1.0 contracts and diagnostic codes live in
`gateway-domain::output_verification`. No provider SDK, second policy engine or
process lifecycle owner is introduced.

## Integration and authority

After transport, load the complete response bytes under the current host's
provenance and disclosure policy. Call `parse_output`, then `verify_output` with
the same immutable IR used for rendering/dispatch and an `OutputVerificationPort`.
The parser accepts a strict envelope with five mandatory fields:

- `schema_version`: `1.0`.
- `invocation`: the full `InvocationIrIdentity` captured at dispatch.
- `result`: JSON governed by the invocation's output contract.
- `references`: JSON-pointer locations and exact `ResolvedReference<ReferenceId>` pins.
- `tools`: proposals containing a pinned capability and JSON arguments.

An output is bounded to 1 MiB and 64 container levels. Malformed/truncated JSON,
trailing data, nested duplicate keys, unknown envelope/identity/reference/tool
fields and unsupported versions are rejected. Deeper inputs exceeding the JSON
parser's own recursion bound may produce `OUTPUT_PARSE_FAILED` before the
explicit depth check. No stream fragment or repair proposal can construct a
`VerifiedOutput`. `ParsedOutput` has private fields and no deserializer.

[#265 parsing contracts](structured-output-parsing.md) now add invocation-bound
schema/tool validation, provisional streams and bounded repair proposals over this
same parser. Provider adapters preserve the dispatch identity and unknown fields
for canonical rejection. Live adapter composition remains separate.

Structural verification compares every invocation identity field, validates the
result with the bound output schema and checks each declared reference's pin,
scope, unique pointer and actual result ID. `resolve_reference` must resolve the
exact pin from current trusted stores. A well-formed pin does not prove existence.
Tool IDs, full pins and argument shapes must match the admitted inventory.
The full IR digest binds task revision, scope, schema/rule contents, provider/model
provenance, authority, sensitivity, generation profile and context/tool snapshots.

`OutputVerificationPort` extends the existing `InvocationAdmissionPort`. The
host authenticates the original task references and current context/authority
before and after output verification. Stale context produces `STALE_CONTEXT`;
revocation or invalid admission produces `AUTHORITY_DENIED`; unavailable
verification produces `VERIFICATION_UNAVAILABLE`. Restore/recompile through the
owning upstream service and create a fresh invocation instead of patching pins.
An accepted record is a snapshot, not a reusable authorization token. External
execution still requires current Process/Policy checks at the effect boundary.

`evaluate` is a mandatory trusted deterministic integration port. Implement it
using current CG-09 Policy, CG-04 Process and task-specific verifiers, returning
explicit `Passed` decisions for policy, process, task completeness and semantic
output. It must inspect the entire payload, including tool arguments, undeclared
references, required uncertainty markers and mutation boundaries. Do not build
these outcomes from model confidence, critics or an output's self-reported
success. There is no permissive production default. Missing/indeterminate
checks cannot accept a result. Reports must echo the exact invocation and raw
output digest; a stale or cross-result report produces `CORRELATION_MISMATCH`.

Required task checks use the pinned rule/verifier definitions already admitted
by CG-10. `assess_result` computes schema validity locally and rejects missing,
extra, failed or indeterminate semantic reports. In particular the host must
scan every mandatory reference, not merely trust the model's reference list.
Evidence/claim checks, where requested, require real reports from the evidence
owner; #267 owns concrete grounding/provenance integration. An absent grounding
implementation must return indeterminate/unavailable. This service does not
interpret raw model text as evidence or implement retrieval.

## Results and operations

`verify_output` returns an immutable `VerifiedOutput` or a typed rejection
record. Both retain the admitted identity, submitted identity and SHA-256 of the
exact response bytes, including whitespace. Correlation failures remain auditable
against both identities. Accepted results retain the parsed payload and tool
proposals; no tool or shell execution follows. Critics are optional audit-only
`CheckOutcome` signals: a negative critic cannot reject otherwise valid output,
and a positive critic cannot override a failed deterministic check.

Retain `VerificationRecord` with the invocation/rendering and response provenance
records under the original scope and sensitivity/disclosure policy. Records
contain identities, diagnostics and critic signals; they omit model response
text and hidden reasoning. A malformed response has no typed identity: correlate
its parser error and raw bytes digest at the host transport boundary. No new
credentials, database schema, launch flags or model runtime is required. Replay
can parse and verify recorded bytes with current stores; it must never redispatch
recorded tool proposals. Rollback removes the additive service and its host call
sites together. Never bypass verification to recover from a rejection.

## Requirement and delivery review

Base: `b7c9540860bffd0f8e1229a995be453901687931`, clean initial checkout,
DEVELOPMENT mode. Requirements: #266, full #178 including architecture additions,
#265 dependency, ADR-007/008/015, semantic-output and policy authority contracts.
Review used requirement, architecture, automation and tester role passes by the
implementing agent; no independent reviewer or approval is claimed.

Readiness: READY_FOR_WORKFLOW for the additive deterministic verification service
and its strict JSON prerequisite. Ordered implementation: reuse admitted IR and
schema contracts; add strict parsed-result boundary; enforce structural/current
host checks and correlated constraint reports; qualify denial paths and retain
evidence. Estimated effort using these foundations: 1–2 engineer-days for
implementation including the parsing prerequisite, 0.5–1 for qualification;
moderate confidence. Live provider/store compositions are downstream work.

| ID | Requirement/source | Production owner/path | Dependencies and state | Automated evidence level | Status |
| --- | --- | --- | --- | --- | --- |
| V01 | #266 schema and task completeness | Domain structural gate, `assess_result`, mandatory completeness/semantic decisions | Existing typed output/task contracts; strict parsing prerequisite included | Schema rejection, malformed/duplicate/unknown/oversize/deep output; every constraint axis; required semantic reports | VERIFIED (component) |
| V02 | #266 unsupported references | Exact pin/pointer gate, current `resolve_reference`, task-wide semantic checks | Current-store host required; no permissive default | Invalid scope/revision/ID/pointer, duplicate pointer, unsupported stored revision | VERIFIED (synthetic host integration) |
| V03 | #266 changed context and constraints | Existing admission before/after checks, mandatory Process/Policy decisions | Current Process/Policy authority remains host-owned | Stale/unavailable/revoked host; context changed during evaluation; denied/indeterminate constraints | VERIFIED (synthetic host integration) |
| V04 | #266 advisory critics | `verify_output` ignores critic signals for acceptance | Mandatory deterministic gates always required | Failed/indeterminate/no critics accept valid result; positive critic cannot override denial | VERIFIED (component) |
| V05 | #266 exact invocation/task revisions | Full IR identity plus exact bytes digest, correlated `ConstraintReport` | Existing #263 immutable IR binds full task/context/provider/tool basis | Every identity field changed, changed inventory, stale output report, accepted/rejected audit correlation | VERIFIED (component) |
| V06 | #178 tool proposals stay data | Exact inventory binding and argument schema; no dispatch dependency | #265 broader parsing/repair and #271 host effects remain separate | Unknown/changed tool pins and invalid arguments; architecture dependency guard | VERIFIED (component) |
| V07 | #178 >=95% and traceability/docs | Per-file measured gate, architecture/CI integration | Rust/Python repository gates | Domain verification 100%; application verification 98.08%; modified invocation modules 100%; architecture and regression qualification | VERIFIED; [evidence](evidence/EPIC-06.06-output-verification.json) |

Run `cargo test -p gateway-application --test output_verification --test prompt_ir --locked`
and the EPIC-06.06 entry in `scripts/quality-gates.json`. Frozen diagnostic fixture:
`tests/fixtures/output-verification-v1/diagnostics.json`. Tests use real canonical
producer construction and synthetic trusted stores. No installed-host/provider,
external-effect verification, complete #265/#267 delivery or parent-epic completion
is claimed. Production hosts must supply the explicit ports before deployment.
