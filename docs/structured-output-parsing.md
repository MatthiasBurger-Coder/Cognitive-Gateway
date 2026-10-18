# Structured output parsing and tool proposals — #265

`gateway-application::output_parsing` consumes provider-neutral response bytes
against the immutable admitted `InvocationIr` from #263. It reuses #266's
`OutputProposalData`, `ToolCallProposal`, strict complete-JSON parser and
`ParsedOutput`. Rust owns validation. Provider SDKs and wire normalization belong
to outer adapters; no tool or shell executor is available in this service.

Call `parse_invocation_output(&ir, bytes)` for complete responses, then pass its
result to `verify_output` with the same IR and current trusted host. The existing
low-level `parse_output` remains compatible for #266 consumers; it checks wire
shape only. A `ParsedOutput` is provisional and has private fields and no
Deserialize implementation. Only `verify_output` can produce `VerifiedOutput`.
Even verified proposals require fresh Process/Policy authorization at dispatch.

The canonical 1.0 envelope requires `schema_version`, the full `invocation`
identity, `result`, `references`, and `tools`. A result-only response uses empty
tool/reference lists. Tool proposals carry the full pinned `capability` and typed
`arguments`. Empty result semantics are defined by the admitted output schema;
there is no implicit null result or success for prose. The parser rejects:

- incomplete/malformed JSON, trailing data, invalid UTF-8 and duplicate members
  at any depth, including escaped spellings of the same key;
- unknown envelope/identity/pin/tool fields, unsupported envelope/identity versions;
- responses over 1 MiB or 64 container levels (including the envelope);
- result fields/types outside the bound closed `OutputType` algebra;
- changed invocation identities or tool IDs/full pins/argument types outside the
  exact captured inventory, including task/scope/revision via the full IR digest.

Unknown result and argument fields are rejected recursively by the existing
closed schema algebra. No implicit coercion or extraction of JSON from Markdown
is performed. Extremely deep JSON may hit serde's recursion bound and report
`OUTPUT_PARSE_FAILED` before the explicit depth check. Diagnostics reuse #266's
stable codes. Exact input bytes, including whitespace, determine the digest.
Reference pin structure/location is checked here; existence, evidence, freshness
and policy remain #266/#267 trusted-host checks.

## Streaming and adapter integration

Create `OutputStream::new(&ir)` per attempt, push byte chunks in transport order,
and consume it with `finish(OutputCompletion::Complete)` only when the provider
signals successful completion. UTF-8 may span chunks. Fragments expose neither
payloads nor tools. `Interrupted` rejects even syntactically complete prefixes;
missing terminal markers must map to interrupted. Overflow clears accumulated
bytes and permanently poisons the stream. A final full JSON parse, schema check
and inventory check always runs. Repeated completion is prevented by ownership.
Adapters must preserve unknown fields for rejection and correlate the original
dispatch identity. They must never execute provider-native tool fragments.

## Bounded repair proposals

Create one `OutputRepairSession` for the original IR with 1–3 attempts and a
positive response ceiling no greater than 1 MiB. `propose` recomputes the failure
from the exact response bytes. Only malformed JSON, invalid result schema or
invalid tool arguments/inventory can produce repair proposals. Valid responses,
version/correlation/reference failures and explicit size/depth limit diagnostics
are ineligible.
No model text is included in the serialized 1.0 proposal: only the original
identity, failed-response digest, diagnostic, attempt and byte ceiling.

The mandatory `OutputRepairBudgetPort::reserve` must charge the original aggregate
session for the entire repair invocation (input/output tokens, time and cost)
and check current authority. There is no permissive implementation. A rejected
reservation consumes a local attempt; rebuilding the local controller cannot
reset the trusted host's session ledger. Repair errors are `INVALID_BUDGET`,
`BUDGET_EXHAUSTED`, `REPAIR_NOT_PERMITTED`. A repair proposal is data and cannot
invoke a model, authorize an effect or construct verified output.

The integrating host enforces the proposal's byte ceiling while receiving the
repair response, reserves generation limits in the unchanged effective invocation
profile, and passes the complete repaired bytes through this parser and #266
again. It retains the original schema, task, scope, sensitivity and tools; a
repair cannot expand them. #268 owns concrete cumulative session accounting and
retry orchestration; #269/#279 own provider/installed-host composition. Missing
budget integration denies repair. This slice provides the contracts and mandatory
port, with synthetic-host evidence; it does not claim those hosted capabilities.

Retain diagnostics and correlated digests under the original sensitivity and
provenance policy. Do not log fragments or model-generated commands as operational
instructions. No database migration, credentials or launcher flags are needed.
Rollback removes additive parsing/stream/repair callers together, preserving
#266's verification gate. Replay parses and revalidates without dispatch.

## Requirement and delivery review

Inspected base `d6ba64473890a598cf1d320eb3fa1fed3c7ecbee`, clean initial worktree,
DEVELOPMENT mode. Sources: #265 and full parent #178 including later additions,
#261/#263 contracts, existing #266 implementation, ADR-007/008/015 and semantic
output contracts. Requirement, architecture, automation and tester role passes
were performed by one implementing agent; no independent approval is claimed.
Readiness: READY_FOR_WORKFLOW. Ordered plan: reuse strict JSON boundary; validate
schema/inventory against admitted IR; add provisional streams and bounded repair
port; qualify regressions/coverage and retain evidence. With these foundations,
estimate 1–2 engineer-days implementation and 0.5–1 qualification, moderate
confidence; concrete provider/session hosts excluded and tracked above.

| ID | Requirement/source | Production path/owner | Dependencies/state | Automated verification | Status |
| --- | --- | --- | --- | --- | --- |
| P01 | Complete versioned bounded JSON, #265 | #266 strict parser → parse_invocation_output | #261/#263/#266 implemented | parser_rejects_fragments_duplicates_unknown_fields_versions_and_limits; invocation_parser_uses_complete_schema_inventory_and_fixture_results | VERIFIED, component |
| P02 | Exact schema and inventory arguments, #265/#178 | parse_invocation_output → shared validate_tools | Admitted immutable IR | invocation_parser_uses_complete_schema_inventory_and_fixture_results; unsupported_references_and_tool_proposals_are_rejected; nested_typed_arguments_reject_unknown_null_and_coerced_fields | VERIFIED, synthetic IR |
| P03 | Provisional streams, #178 | OutputStream push/finish | Adapter must signal completion | streams_require_completion_and_overflow_is_terminal | VERIFIED, component |
| P04 | Bounded repair proposals, #265 | OutputRepairSession → mandatory budget port | Concrete aggregate ledger owned by #268 | repairs_are_correlated_bounded_and_require_aggregate_admission | VERIFIED, synthetic host port |
| P05 | No raw text/commands/tool dispatch, #265 | Pure parsing service → #266 verification | Current Process/Policy remains mandatory | Existing constraint rejection tests and architecture dependency/effect checks | VERIFIED, component |
| P06 | Documentation, >=95% coverage, traceability | Operator/arc42 docs and CI gate | Existing llvm-cov tooling | scripts/check-output-parsing-coverage.py and retained evidence | VERIFIED, retained qualification |

Evidence: [EPIC-06.05 qualification](evidence/EPIC-06.05-output-parsing.json).
These criteria qualify the requested parsing contracts, not the full parent epic
or a live provider/tool execution path.
