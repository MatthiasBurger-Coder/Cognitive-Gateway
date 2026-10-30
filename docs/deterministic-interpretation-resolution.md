# Deterministic entity, reference and context resolution

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

EPIC-05.09 [#187](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/187)
implements the request-scoped resolver in
[`gateway-application::interpretation`](../crates/gateway-application/src/interpretation.rs)
and the typed snapshot in
[`gateway-domain::interpretation`](../crates/gateway-domain/src/interpretation.rs).
It consumes the normative [Interpretation Context](interpretation-context.md)
and [resolution result contracts](semantic-resolution-contracts.md).

## Delivered API and ownership

1. A trusted host constructs `InterpretationContextData`, retaining input,
   conversation/task bindings, exact source pins, source permissions, typed
   relevant facts, registered aliases and temporal anchors.
2. `InterpretationContext::admit` (or `admit_json`) authenticates the binding
   before inspecting context. It validates version, scope, half-open lifetime,
   pins, duplicate identities, source bounds and correction relationships.
   The host must authenticate the complete projection before it is admitted.
3. `DeterministicInterpretationResolver::resolve` takes a typed field query,
   expected meaning, original input provenance/span and explicit evaluation time.
   It validates the query against the pinned input, revalidates the capture,
   and resolves through registry and project `InterpretationLookup` ports.
4. The result uses the existing `SemanticResolution<InterpretationValue>`.
   `require_resolved` exposes only unique meaning. The versioned decision
   envelope retains the query, request/context/revision, capture digest,
   evaluation time, original provenance, exact lookup results and rule trace.
5. `revalidate_decision` repeats reference eligibility and exact lookup checks
   before consumption. A changed basis, lookup result, permission, host binding
   or evaluation time rejects the old decision. A new evaluation requires a
   new decision; a changed snapshot basis requires recapture.

`InterpretationHost` is a mandatory trust boundary, not a permissive default.
Implementations must validate exact source content/revisions, epistemic and
memory eligibility, provenance, origin tiers, source-access limits, authenticated
conversation/task associations, correction relationships and calendar projections.
They must validate entity existence/kind/revision/digest through `validate_fact`.
IDs and shape validation alone do not establish these properties. Query text
cannot change the admitted project. A project switch needs a separately admitted
host request. Admission errors contain no foreign IDs or payloads.

`CapturedInterpretationLookup` supplies a concrete replay adapter keyed by the
exact scope, source permission record and query. Hosts can provide captured
registry and project results separately, or implement live adapters with the same
bounded exact lookup contract. Empty/unavailable acquisition never synthesizes a
value. The resolver performs no network retrieval and reads no system clock.

## Resolution rules

The source ordering is current input, task state, active situation, conversation
focus, verified memory, retrieved evidence. Clarification is an unresolved result
for the later clarification planner; it is not a source tier with default values.
All fields resolve independently. `subject` associates dependent state/history
with an exact target binding; facts for target A cannot satisfy fields for B.

Explicit canonical IDs bypass alias spelling. Aliases match exact surface label,
kind and admitted scope; their values are direct target bindings, so alias chains
cannot be represented. An explicit missing alias remains unknown, and an explicit
unknown ID is invalid. No task/focus fallback replaces either. An explicit alias
with several eligible bindings returns all candidates. Confidence, recency and
lexical ordering never select a winner.

For discourse queries, the first eligible tier wins. Equal exact meanings merge
their provenance; conflicting entity revisions stay separate and conflicting.
Multiple explicit/task/situation declarations conflict, while several possible
conversation referents remain ambiguous. A validated correction relationship can
supersede a declaration. Protected immutable task meaning or established current
truth cannot be overwritten by a higher tier. Stale implicit context is excluded
with a reason; invalid explicit/required pins block the field.

Supported temporal expressions are signed integer seconds relative to a named
absolute instant and `Yesterday` relative to a named Gregorian civil-day anchor.
The latter requires a recorded timezone and the exact previous-day half-open
interval. The trusted host owns timezone/calendar projection, including DST and
offset/fold choices. No fixed 24-hour subtraction is used for civil days.
Missing calendar information, unresolved multiple anchors and arithmetic
overflow remain unresolved/ambiguous. “Still” is represented by a typed focus
query plus relevant observation/action reference facts; it does not infer a
current measurement or repeat a completed action.

## Canonical encoding and boundaries

Snapshot wire version is `1.0`; unknown fields fail admission. Relevant facts,
aliases, anchors, source permissions and basis pins are canonical sets. Identical
records deduplicate; duplicate identities with different content are rejected.
Literal sets are sorted and duplicate values rejected. Canonical snapshot bytes
are compact UTF-8 JSON of the admitted, normalized typed record; its SHA-256
digest is separate from SemanticTaskIR's digest. Decision `canonical_bytes`
uses a separate versioned inspection envelope, including evaluation time.

The [CGSL frontend](cgsl-frontend.md) continues to produce typed operands and
inspection sidecars. A host projects these operands and natural-language
references into the same resolver queries; aliases do not modify CGSL grammar.
No new provider, authority model, process engine or session coordinator is added.
This issue delivers an application service and adapter contracts with component
evidence. It does not install an end-user natural-language command. Completeness,
clarification planning and executable planner handoff remain #188/#190; durable
pause/resume/cancellation and reply correlation remain host/#275 responsibilities.
Semantic resolution does not satisfy policy or consent checks.

## Requirement review and evidence

The implementing agent performed requirement, architecture, automation and test
role passes against baseline `59edfd2eae40a008afcd82d22160ced58b6f843f` plus this
working change. This is a single-agent review, not independent approval.
#181/#182 contracts and the delivered #186 frontend were inspected directly.
The implementation order was snapshot/host admission, resolver/ports, adversarial
fixtures, then coverage and regression checks. The required evidence level for
this service issue is COMPONENT; full installed-client/runtime proof belongs to
the later integration issues. Readiness decision: READY_FOR_WORKFLOW.

| ID | #187 requirement | Production path / owner | Evidence | Status |
| --- | --- | --- | --- | --- |
| R1 | Supported inputs need no LLM | Application resolver and captured lookup adapter | Exact IDs, aliases, focus, reference/literal fields, temporal fixtures | VERIFIED |
| R2 | #181 precedence | Tier selection, corrections, protected conflicts, target association | All six tiers and reversed collection order; explicit failure and target override cases | VERIFIED |
| R3 | Ambiguity exposes candidates | Existing domain candidate sets | Alias/focus/anchor ambiguity; exact-binding provenance merge; revision conflicts | VERIFIED |
| R4 | #182 failure contracts | Existing SemanticResolution and SemanticGapReason | Missing, unknown, stale, invalid and conflicting paths; mandatory extraction rejected | VERIFIED |
| R5 | Scope fails closed | Host admission, scoped pins and lookup validation | Foreign entity, session mismatch, source mismatch, wrong kind, bounded lookup tests | VERIFIED |
| R6 | Accepted resolutions explain source/rule | Decision envelope and trace | Canonical decision JSON, input/lookup provenance, exact replay and handoff invalidation | VERIFIED |

Fixtures are in [`tests/fixtures/interpretation-v1`](../tests/fixtures/interpretation-v1/README.md).
The integration tests exercise the real resolver with an injected fixture host;
they do not qualify an external registry/database/session authority adapter.

```sh
cargo test -p gateway-application --test interpretation --locked
cargo llvm-cov -p gateway-domain -p gateway-application --test interpretation --locked --json --output-path target/interpretation-coverage.json
python3 tests/scripts/check-interpretation-coverage.py --self-test
python3 tests/scripts/check-interpretation-coverage.py target/interpretation-coverage.json
```

CI runs the same coverage command and requires at least 95% measured line coverage
independently for both new production modules. Reports are retained by the existing
quality runner. Coverage and component results do not establish full EPIC-05
acceptance or authorize issue closure.

### Local verification, 2026-10-16

- Resolver suite: 18 passed. Domain/application/CGSL regression selection:
  582 passed, zero failed or ignored.
- New domain module: 14/14 covered lines (100%); application resolver:
  561/570 covered lines (98.42%). Both pass the per-file 95% gate.
- Workspace build, formatting, dependency/architecture guard, coverage-gate
  self-test and whitespace checks passed.
- Full workspace tests remain blocked by the unchanged daemon test
  `local_mcp::runtime::tests::active_cancel_disconnect_errors_and_deadline_are_bounded`
  (`runtime.rs:545`, missing context after the 5ms timeout). It failed on both
  workspace attempts; the affected crate regression selection above is green.
- Strict Clippy remains blocked by the existing `nonminimal_bool` diagnostic at
  `gateway-application/src/worker_fabric.rs:291`; no resolver diagnostic remains.
- The architecture unittest suite ran 35 tests and reported five errors: one
  offline-learning benchmark subprocess failure, three source-map `KeyError`s
  and one missing installed-qualification report. The same five errors were
  reproduced in a clean detached worktree at the inspected baseline.

The [source-bound evidence summary](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-05.09-interpretation.json)
records this component result and the broader qualification failures. These
failures were not bypassed or repaired as part of the resolver change. Full
repository qualification is not claimed.
