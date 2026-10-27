# EPIC-08.03 #274 — Three-Amigos delivery gate

Candidate inspected: `742c318b47b1e06ced615618fe7ac0c1d05f71ce` plus the
uncommitted #230/#274 component changes in the working tree. GitHub issue #274,
parent #271, prerequisite #273, contract issue #272, and cross-plane additions
to #230 were read on 2026-10-25. The user accepted the scoped snapshot rule
recorded below during this review.

## Review provenance and decision

Requirement, architecture and test/evidence reviews were performed by three
separate subagents. Their findings were reconciled by the coordinating agent;
no role is represented as a human approval.

**Decision: REQUIRES_REFINEMENT for shipped delivery.** The user resolved
snapshot identity and phase semantics. Implementation preserves the existing
`SupportedArtifactGoal` and adds distinct trusted connector-task registration
for goals with planned connector actions and independent observation sources.
Application contracts and local host registration now implement this
separation. Dispatch through the installed local session runtime remains
unsupported and is an open delivery gate.

## Accepted connector-step snapshot contract

Each dispatched plan step has an immutable, persisted `Before` and `After`
snapshot. Both bind authenticated owner (including project), canonical scope,
session, run, session revision, task, plan step, dispatch and invocation
identity. Connector observations are separate immutable records. A provider
acknowledgement is never the independent goal-verification receipt. Snapshot
payloads and references must follow existing scope, provenance, sensitivity,
size and content-addressing rules. The existing #276 fenced journal owns atomic
reservation/result persistence and recovery; #274 adds no second journal.

## Acceptance matrix

| ID | Requirement/source | Production path/owner | Dependencies and actual state | Implementation evidence | Required verification / evidence level | Status |
|---|---|---|---|---|---|---|
| R1 | Unknown tool, schema mismatch, changed inventory, policy denial, stale consent produce zero dispatch. #274; #230 cross-plane addition | Invocation admission → verified output → proposal binder → CG-09 connector application → transport | Application composition requires exact target/inventory revision and argument-schema digest to match live discovery; CG-09 allow/deny/revalidation tests exist. Local host registration admits only trusted pinned connector tasks; `LocalConnectorRuntime` still returns `UnsupportedCapability`, so no installed composition or production `ConnectorPolicySource` uses these checks | `connector_proposals.rs`, `connector_policy.rs`, `mcp_client/host.rs`; `output_verification.rs`; `local_sessions.rs` | Denial matrix through shipped dispatch composition, asserting zero transport calls; EXECUTABLE | OPEN |
| R2 | Connector success alone cannot satisfy the goal. #274; #271 | Separate admitted post-effect read → `ConnectorResult` → existing `ClosedLoop` → immutable evidence | Distinct final `GoalVerificationReadback` records are encoded, owner-scoped reloaded and complete-set validated. The two-step coordinator test persists a separate goal readback and remains incomplete when no verified outcome is available. No shipped runtime/CG-09 source produces it | `connector_results.rs`, `sessions/connector_execution.rs`, `sessions/readback_storage.rs`, `sessions/coordinator.rs`; `session_runtime.rs` | Fresh complete condition readback after the last step; owner-scoped reload, exact mapping/lineage validation, acknowledgement-only and stale/wrong-state negatives; durable DB restart; EXECUTABLE + SHARED_SERVICE | PARTIAL |
| R3 | Two-step workflows retain complete scoped snapshots. #274 | Existing `SessionCoordinator` → #276 journal → owner-scoped immutable records | Synthetic SessionCoordinator test asserts four before/after refs bound to owner/project/scope/session/run/revision/step/dispatch. A separate two-process test reloads snapshots, proposal, result and observation from PostgreSQL-backed storage, then recovers as Unknown without redispatch. This does not qualify a PostgreSQL server restart or actual provider effect; shipped host composition remains absent | `session_runtime.rs`; `session_contracts.rs`; `session_store.rs`; `session_store_tests.rs` | Two sequential steps, payload reload across process restart, PostgreSQL server restart and shipped runtime proof; SHARED_SERVICE | PARTIAL |
| R4 | Partial and uncertain outcomes are typed. #274; #271 | Coordinator checkpoint / dispatch knowledge / recovery | Batch result is versioned, content-free and derived from typed `ProposalBatch`; runtime errors, invalid turns and successful turns with typed Unknown all independently request After capture, or persist explicit Missing with Unknown. Later-step uncertainty preserves first-step records. Recovery over retained in-memory repository does not redispatch. PostgreSQL restart/reconciliation and shipped session path remain unqualified | `ProposalBatch`, `ConnectorResultRef`, `ConnectorEffect`, `DispatchKnowledge`; `session_runtime.rs`; `sessions/journal.rs` | Later-step failure retains earlier records and either a bound After snapshot or typed Missing; unknown blocks replay; PostgreSQL journal restart/reconciliation proof; EXECUTABLE + SHARED_SERVICE | PARTIAL |
| R5 | Proposal invocation is production-admitted through coordinator. #274 dependency #273 | Shipped session v2 `LocalApplication` → shared `SessionCoordinator` → EPIC-06 invocation → EPIC-07 mapped host | Trusted connector goal registration exists in `SessionAdmission`; `InvocationHost` and `LocalConnectorRuntime` still decline dispatch, and outbound MCP host has no local-session composition | `local_sessions.rs`; `invocation_host/session_tools.rs`; application runtime and MCP components | Actual shipped CLI/MCP session flow with current admission, connector mapping and runtime; EXECUTABLE | OPEN |

## Dependency ledger

| Owner / issue | Inspected implementation | Required contract and gate | Consumer / order |
|---|---|---|---|
| EPIC-08.02 / #273 | Shared coordinator/PostgreSQL, trusted connector registration and fenced phase preparation exist; `LocalConnectorRuntime` remains unsupported | Compose the current producer/policy/connector/readback hosts in the same lifecycle | #274; before shipped-path acceptance |
| EPIC-06 / #265–#268 | Tool-capable invocation components and reserved `SessionToolRun` are qualified; standalone `InvocationHost` stays limited to its registered read-only scope | Install the exact current plan/inventory producer and live joined invocation/resource authority | #274; before shipped coordinator dispatch |
| EPIC-07 / #227–#231 | #227–#229 closed; #230 and #231 remain open. Connector mapping/runtime/result components exist; #230 adapter is component-only | Compose current consent/policy/inventory admission and a concrete mapped connector host; immediate revalidation remains the transport precondition | #274; before any transport effect |
| EPIC-08.05 / #276 | Fenced immutable snapshot/observation and v2 phase-plan records persist across writer/reader processes | Qualify database-server restart and controlled external-effect recovery through the shipped runtime | #274 snapshot/result transitions; full durability acceptance |
| EPIC-08.06 / #277 | Typed conservative reservations, v2 phase claims and model child-run enforcement exist | Compose concrete connector/retrieval/worker enforcement, CG-14 retry/replan/no-progress and global overload limits | Complete parent acceptance remains required |
| EPIC-08.08 / #279 | System qualification is a separate parent-epic issue | Live controlled external effect qualification is required for #271, not silently claimed by the #274 child component report | Parent EPIC only |

## Ordered delivery slices

1. Add immutable scoped connector snapshot and observation references to the
   existing journal contract and PostgreSQL adapter. Preserve old checkpoint
   decoding and current artifact-task behavior.
2. Add a distinct host-registered connector-task goal. Registration pins the
   CG-14 Intent, plan/steps, scope, capability mappings and independent
   observation mappings. No client/model field can set these values.
3. Enable a tool-capable EPIC-06 invocation only for that registered current
   step; bind full mapped inventory schemas and exact IR. Unknown or stale
   inputs fail before transport.
4. Extend the existing session coordinator to reserve and persist a `Before`
   snapshot before dispatch, then persist ordered observations and an `After`
   snapshot under the same owner/run/step/dispatch. Retain earlier results on
   later failure; mark ambiguous effects unknown.
5. Use a separate, admitted post-effect read to build a trusted
   `ConnectorResult`/`ScopedObservationBatch`; pass it into the existing
   `ClosedLoop` and store verified evidence only when the original goal is
   satisfied.
6. Compose this path in the shipped session adapter and qualify denial,
   two-step, partial, restart and goal-verification cases. Refresh source-bound
   reports and hashes against the final candidate before any issue closure.

The full implementation/qualification effort is provisionally **8–18
engineer-days**, excluding external client/provider availability. This range
includes the #273/#276 contract changes necessary for the issue's own criteria;
confidence is low until the model/schema mapping and operator configuration
shape are confirmed by executable qualification. This is engineering effort,
not elapsed calendar time. Parent-level live system qualification remains #279.

## Current evidence boundary

The connector phase follow-up changes the candidate after the final15/final16
runs described below. Those runs remain historical evidence; they do not
qualify the new adapter or changed transport guards. Fresh all-target coverage
is required, including `mcp_client/session_budget.rs` in the session gate.
The new controlled-peer component suite passed 10 tests with actual coordinator
allocations and CG-09, including conservative spend, exact reservation binding,
readonly recovery, late revocation, sticky metric failures and bounded audits.
Independent review then found discovery effect-classification and current-target
polling gaps; corrections and their regression qualification are in progress.
Installed composition, current production authority and live effects remain
open regardless of these component results.

The production resource-claim lookup now uses authenticated current workspace
configuration and the existing PostgreSQL session store. Thirteen local-host
tests pass, including a real coordinator-persisted claim accepted by that
production method and rejected after config drift or persisted stop/fence
change. Joined operation admission remains default-deny pending configuration.
The final transport suite passes 52 library and 22 integration tests; model
session tests pass 33, and application policy tests pass 11.

The next production-policy review identified missing canonical step inputs in
the current snapshot (`resolved`/prerequisites/constraints/preconditions) and
missing immutable evaluation context in the audit callback. These must be
refined before installing a production policy source. The agreed composition
uses shared current authenticated access without a Host/Runtime ownership
cycle, actual canonical policy facts and a bounded separate audit journal that
does not change the active session claim. Fresh source-frozen all-target runs
are in progress under `target/epic08-session-qualification-20261026-final18/`;
no full candidate qualification is claimed yet.

### Canonical policy follow-up

The snapshot now requires actual step identity, resolution state, prerequisite
presence and constraint/precondition sets. The existing PolicyEngine consumes
them directly. The read-only audit envelope retains the captured snapshot,
optional report, original pre-effect dispatch and effective allow/deny result;
early contract failures have no invented report. A successful pre-effect audit
is followed by one audit-free current check, closing the confirmed standalone
host callback-revocation window. Sixteen policy tests and 37 proposal/output
tests pass; 12 real-CG09 connector cases include the unguarded standalone API
denial and late quote changes. Four transport-context tests cover guard order,
shortcircuit and post-callback cancellation/deadline checks.

Production prerequisites now share `Arc<LocalSessionAccess>` without a
Host/Runtime cycle. Fifteen PostgreSQL access tests and nine existing shipped
CLI/MCP host tests pass. Canonical policy mapping needs no context projection;
its three mapping tests, two CLI policy regressions and walkthrough pass.
`PolicyApplication::step_input` and normal evaluation use one native assembly;
12 tests retain complete capability closure and exact step/process constraints.
These focused results do not replace current full coverage qualification.

Independent architecture review accepted immutable per-source audit provenance:
the exact same source instance captures the complete static canonical/runtime/
manifest/mapping/producer basis once, validates every current lookup against it,
and builds the snapshot from that same loaded owned data. Drift fails before a
snapshot is returned. The audit callback combines that immutable basis with
the actual envelope; it never reloads supposed earlier provenance. Mutable
facts are projected from the supplied snapshot. Allocation audit binds both
reservation revision and claim revision, fence, phase, amounts and cost unit;
an audit append ordinal does not claim a unique child-allocation mint identity.
The durable audit implementation is in progress and remains unqualified until
its PostgreSQL invariants and source composition pass.

Coverage and tests qualify components only. The final15 Application and
final16 Daemon all-target runs passed with the disposable PostgreSQL host.
Their reports are retained under the corresponding
`target/epic08-session-qualification-20261026-final15/` and `final16/` paths and
archived in the source-bound record. The expanded session gate includes the
reserved model adapter: `session_tools.rs` 410/418 (98.09%), `resources.rs`
610/619 (98.55%), `coordinator.rs` 1555/1633 (95.22%), `journal.rs` 445/454
(98.02%), `session_store.rs` 862/906 (95.14%), `local_sessions.rs` 1165/1211
(96.20%). Every selected module passes >=95%; the separate HTTP-model and
invocation-resilience gates pass too. The resilience gate uses its prescribed
Domain+Application instrumented report, rather than treating the Application-only
report's missing Domain entry as a pass.

Focused suites passed 22 contracts, 14 resource tests, 49 session runtime tests,
13 invocation-resilience tests, 33 model-session tests and seven PostgreSQL
resource tests. The PostgreSQL writer/reader processes retain distinct v2
preparation/resume amounts. Workspace Clippy, architecture guard, format and
whitespace checks pass. Architecture regressions: 83 tests with one native
Windows-only skip; JSON contracts: 26 tests. The real-coordinator test harness
uses no public capability factory. Linux fixture imports no longer embed the
current checkout's absolute path.

This does not qualify database-server restart, installed connector runtime,
production authority composition or controlled live effects. R1/R5 and
shipped-runtime portions of R2–R4 remain OPEN/PARTIAL.

The full parent #271 acceptance status is tracked separately in
[`EPIC-08-parent-acceptance.md`](EPIC-08-parent-acceptance.md); this component
review does not imply parent acceptance.
## Follow-up shipped-path audit

The daemon-only composition review found no safe existing runtime that can be
installed through `LocalSessionHost::connector_runtime`:

* `SessionAdmission` now supports distinct artifact and trusted connector-task
  goal registration. Registration pins intent, plan, ordered steps,
  target/schema mappings and independent observation mappings.
* `InvocationHost` is intentionally limited to Inspect/Analyze with no
  approved connector capabilities. It admits an empty tool schema and rejects
  an invocation containing tools. Relaxing those checks globally would widen
  the existing standalone host's authority.
* The separate `SessionToolInvocation`, `SessionToolVerificationHost`,
  `SessionToolHttpRenderer` and `SessionToolHttpAdapter` implement tool-capable
  invocation components. Their existence does not supply the missing current
  session authority or install them in `LocalConnectorRuntime`.
* There is no production `ConnectorPolicySource`. `Cg09ConnectorAdmission`
  requires an operator-owned source that binds exact request arguments,
  current inventory, project/session execution and live consent; existing MCP
  tests use injected fixtures and do not provide that source.
* No `ConnectorSessionRuntimePort` implementation currently joins current
  plan/step preparation, tool-capable invocation and output verification,
  `ConnectorProposalDispatcher`, `McpConnectorHost`, independent post-effect
  reads and the session journal.
* The product-facing daemon `LocalConnectorRuntime` confirms this boundary:
  initial assessment is explicitly `UNKNOWN`, while `prepare_step`,
  `execute_step`, and `verify_goal` return `UnsupportedCapability`. The local
  Codex configuration inspected for this qualification has an empty
  `mcp_servers` table, and the repository contains no operator target/server
  registration or live CG-09 source that could be installed there.

This cannot be completed by wiring the existing `McpConnectorHost` into the
daemon alone. Registration exists, but runtime composition still needs
operator-owned connector/provider configuration, a live `ConnectorPolicySource`
for consent/policy/budget/inventory, composition of the existing tool-capable
invocation components returning only `VerifiedOutput` for the exact IR,
complete scoped snapshot capture and independent condition readbacks.
The configured runtime, current authority adapters and concrete connector/
retrieval/worker resource enforcement remain repository implementation work.
The conservative session resource ledger and fenced preparation/resume claims
are now implemented as detailed below. Missing operator
configuration blocks controlled live qualification, but does not prevent that
implementation. The runtime remains fail-closed until these components are
implemented and configured. Until composed and synthetic-MCP
qualified, R1/R5 and shipped-runtime portions of R2–R4 remain OPEN. No stub or
artifact-host behavior is evidence of execution.

## Follow-up 3-Amigos resolution: provider model binding

Independent architecture/automation and requirements/test reviews identified a
configured-provider defect in the existing tool-capable invocation path:
`SessionToolHttpAdapter` accepted an operator runtime model mapping, but
`SessionToolHttpRenderer` could not render that mapping. The adapter also
returned the provider name to a verification boundary that requires the
canonical model identity.

`SessionToolHttpRenderer::for_runtime` now validates the complete canonical
model reference and provider name, renders the provider name before token
accounting, and identifies that rendering as template version `2`. The
tokenizer profile retains the canonical model pin. The HTTP adapter requires
the matching template version and reconstructs the exact mapped body before
any HTTP call. After checking the response's exact provider model name, it
normalizes the completion to the canonical model identity for verification.

`cargo test -p gateway-daemon --test session_tool_invocation
session_http_runtime --locked -j1` passed all three tests. The scenarios reach
`VerifiedOutput` through loopback HTTP, reject a wrong response model, reject
changed canonical pins and invalid provider names, and prove zero TCP
connections when renderer and adapter mappings disagree. Modern and legacy
Chat Completions rendering retain the canonical accounting pin. Independent
read-only review of the implemented slice found no blocking issue. This is
component evidence with a controlled HTTP server; installed session runtime
composition and live qualification remain open.

### Cumulative reservation foundation and remaining runtime integration

The follow-up architecture pass inspected #277, `SessionBudget`, the coordinator,
the session journal/store and existing model/repair/retrieval/worker budgets.
The repository-owned sequence below does not require live credentials for implementation. Steps 1–3 and the model portion of step 4 have component/storage evidence; the remaining adapter and CG-09/runtime work in steps 4–6 stays open:

1. Add typed session resource limits, cumulative usage and immutable reservations,
   reusing existing model/repair/retrieval/worker units. Missing resource
   admission denies resource-consuming routes; unknown metrics retain the
   full admitted upper bound. Use checked arithmetic and immutable limits.
2. Persist the ledger with the existing owner/project/scope/session/run
   checkpoint and fence. Reject consumption rollback, reservation replacement,
   removal and limit increases. Legacy artifact checkpoints cannot implicitly
   gain connector/model resource admission.
3. Persist a trusted bounded whole-step reservation before
   `prepare_authorized_step`, including model attempts/repair and all snapshot
   and goal reads. The resource reservation now precedes preparation; action reservation remains at connector dispatch.
   Distinguish resource-consuming preparation from connector mutation send so
   recovery cannot mistake a paid model call for an external mutation.
4. Give nested adapters disjoint allocations from that durable reservation;
   retry/fallback/replan/restart cannot refill it. Retain conservative full-step
   charges initially, including consent pauses and absent telemetry.
5. Bind `ConnectorDispatch.budget_reservation` to that exact persisted envelope.
   The production CG-09 source must load and revalidate it together with current
   request/owner/run/step, deadline, cancellation, grant and policy.
6. Qualify failed append with zero model/read/connector calls, nested exhaustion,
   stale/foreign reservations, PostgreSQL restart/fencing and unknown usage;
   then compose the configured runtime and qualify controlled external effects.

The typed ledger, immutable checkpoint/storage binding, pre-preparation append,
and approved-resume claim are implemented. Application tests cover failed
append with zero runtime calls, exhaustion, no refill, consent retention,
interrupted preparation, early/fresh expiry checks and recovery without replay.
The daemon tests retain the ledger across separate PostgreSQL-backed writer and
reader processes. Independent implementation and requirements/test reviews
resolved the additional expired-consent recovery and concurrent-resume gaps;
no blocking finding remains for this foundation. Nested adapter allocation,
production authority composition and full parent acceptance remain open. The
standalone invocation file journal is keyed differently and cannot replace
the session's fenced journal. The separate no-progress assessment decision
remains unresolved and is not assumed by this reservation design.

The phase-allocation and model adapter slice is now implemented. V2 identities
bind the checked disjoint phase sum; v1 total-only records remain readable and
cannot mint phase capabilities. The coordinator reloads the exact fenced
checkpoint before minting, attaches its claim revision/fence and earlier
lease/session deadline, and passes one cursor to each admitted phase. A failed
append releases no cursor. Snapshot fallback shares one cursor across mutually
exclusive error paths. Goal verification and consent revalidation remain pure;
nonzero goal-verification acquisition is refused until separately claimed.

`SessionToolRun` owns one model-only child allocation and one `InvocationRun`.
Retry/fallback cannot replace its limits/context or refund conservative output
and cost quotes. Current resource authority joins the exact claim to registered
invocation producer pins, including same-scope foreign-producer denial. Count
estimates require positive current qualification of the full profile, pinned
provenance and payload bound. The established UTF-8-plus-framing method has a
positive qualified test; arbitrary or withdrawn estimates refuse before I/O.

Independent reviews found and resolved verification-callback revocation,
post-verification cancellation/deadline, late tokenizer qualification changes,
and the initially separate task/claim checks. Provider admission now follows
host callbacks; after verification the adapter rechecks allocation and joined
producer pins, then performs the original run's final cancellation/deadline
check without further callbacks. The 33-test model suite passed, including all
12 new reserved-run scenarios. Separate PostgreSQL writer/reader processes
retain the v2 plan and its distinct preparation/resume amounts without refill.

Remaining work is concrete connector/retrieval/worker enforcement, the real
current authority sources and shipped runtime composition, followed by the
complete parent/live qualification. This component qualification does not
resolve the separately pending no-progress assessment contract.

## Follow-up 3-Amigos resolution: restart-safe goal verification

Requirement, architecture and test/evidence reviewers independently agreed
that step-1 observations must not be combined with a final-step read as if they
were contemporaneous. After the last planned step, the runtime must acquire a
fresh readback set covering every registered condition-to-source/capability
mapping. Those immutable readback records remain `ConnectorObservation`,
separate from both the proposal-batch `ConnectorResult` record and final
CG-14 `Evidence`.

On verification and after restart, the coordinator must load the exact
owner-scoped readback references, check record kind/digest/contract version,
binding, read-only request, exact mutation linkage where applicable, scope,
freshness, and normalized evidence lineage. It then rebuilds one complete
scoped CG-14 input for the registered intent. Missing, stale, duplicated,
tampered, cross-owner/cross-scope or unmapped observations fail closed. Only a
separate successful `VerifiedOutcomeReceipt` and its `Evidence` record may set
`final_evidence` and complete the session. A two-step PostgreSQL restart test
must prove verification can reload the final readback set without redispatch.

The reviewers also resolved the readback-role distinction: `PostEffectReadback`
remains tied to its exact mutation request and uses
`IndependentPostEffectRead`; a complete final condition set uses a separate
`GoalVerificationReadback` role with an admitted read-only `SourceRead`,
`NoMutation`, its own read request identity, and no `verifies` field. The latter
is never evidence that a particular mutation succeeded. Every final condition
must have exactly one fresh source/capability-matched read in the final
verification epoch, and those extra reads pass current CG-09/consent admission
and count against connector budgets.

The coordinator now stores final condition reads as distinct immutable
`GoalVerificationReadback` records, reloads them through the owner-scoped
repository and validates full condition coverage before ClosedLoop verification.
Recovery can complete this verification from persisted records without another
connector call. A synthetic `ConnectorSessionRuntimePort` now exercises the
actual `SessionCoordinator` lifecycle for a two-step connector goal. It
persists four distinct snapshot records, a proposal result and a separate final
goal readback; an absent `VerifiedOutcomeReceipt` leaves the session
uncompleted. A second scenario makes step two uncertain, confirms step-one
records remain, then constructs a new coordinator over the retained repository
and proves recovery does not dispatch again. These tests improve shared
application qualification for R2–R4. This coordinator recovery uses an
in-memory retained journal. Separately, a PostgreSQL-backed session-store test
launches writer and reader processes and verifies persisted connector records
and fail-closed Unknown recovery across process restart; it does not restart
the database server or exercise a live provider. On runtime error, invalid turn,
or a successful turn carrying a
typed `Unknown` without an After snapshot, the coordinator asks the trusted
runtime to capture an independent After snapshot. A captured snapshot is
persisted beside typed Unknown; if capture fails, `ConnectorResultRef` records
`Missing` and the session remains incomplete. Checkpoint validation requires
the state to agree with the After reference. This satisfies the evidence
contract when capture is available and exposes the missing case without
claiming completeness. The synthetic runtime also does not establish live CG-09
authorization or the shipped daemon InvocationHost/session composition. R1/R5
and the durable restart/live-policy portions of R2–R4 therefore remain open.


## Retained verifier and source-factory follow-up

The current product verifier consumes the immutable owner-scoped PostgreSQL
readbacks and verifies the complete original goal through native
`ClosedLoop::verify_retained`. Its final 23-test local-session PostgreSQL run
includes the real Normalizer→storage→decoder→product-verifier chain, an unmet
goal, weakened-goal rejection, real deadline expiry and a concurrent persisted
cancel. It performs no model or connector call. The focused logs and exact source
hashes are retained in
[the follow-up archive](EPIC-08.03-retained-verifier-followup-validation.json.gz).
The master report records the archive hash and explicitly leaves full acceptance
open. Earlier full coverage and Clippy passes are historical for these sources.
Current strict daemon Clippy fails on three groups of TargetFactory APIs awaiting
real consumers; no lint suppression or fake consumer qualifies them.

| ID | Requirement/source | Production path/owner | Dependencies and actual state | Implementation evidence | Verification and evidence level | Status |
| --- | --- | --- | --- | --- | --- | --- |
| V1 | Original complete goal, no success from provider acknowledgement | Core `verify_retained`, session coordinator, local GoalVerifier | Full goal and internally constructed readset digest required | Native pure verifier and both coordinator receipt checks | Core 21 tests, session runtime 52 tests, product verifier in 23 real-PG local-session tests; component scope | PARTIAL |
| V2 | Immutable policy evaluation audit | PostgreSQL decision journal, future production policy Source | Store implemented; concrete Source installation remains open | Captured audit basis and read-only evaluated envelope, separate append-only journal | Six real-PG tests; no installed source proof | PARTIAL |
| V3 | Complete authenticated native task references | Native SemanticTaskIR and private RuntimeAdmission loader | Read-only native reference API implemented; typed transitive loader closure next | Same reference assembly as native validator, no duplicated field list | Nine native task tests; discovery is not authentication or target authorization | PARTIAL |
| V4 | Real proposal preparation and zero unauthorized effects | Product ProducerFactory/SourceFactory/TargetFactory | Full capability pins, selector-to-task association, current authority, real invocation and collector composition still needed | Component adapters and data loader exist; preparation/execution remain unsupported | Installed multi-step/live proof absent | OPEN |

Review provenance: the retained-readset Core review was independently performed
by the gate-review agent; its initially suspected absent version marker was
withdrawn after the native SourceSnapshot constructor proved that state
unreachable. The normalizer acquisition-quality fix was implemented by the
budget agent and exercised independently by the phase agent's actual PostgreSQL
verifier chain. Root reviewed the read guards and updated the qualification
boundary. The next reference-closure slice receives an independent gate review;
selector/response association is separately under architecture refinement.
Neither discovery nor a matching JSON schema constitutes target authorization.

The executable ordering remains native reference discovery → authenticated typed
closure → target/selector association and full capability pin resolution →
concrete policy/producer/collector composition → complete lifecycle and current
coverage qualification → installed/live and all-parent acceptance. The user’s
accepted immutable snapshot contract and complete #271/#272–#279 scope remain
unchanged. No issue closure is authorized by these component results alone.


### Capability reference source decision

Independent registry/API inspection found no native standalone capability
revision in the current catalog: the index contains native definitions and
provider candidates, and the resolution basis pins the registry as a whole.
Consequently the product must not synthesize a capability reference revision
from an ID, registry fingerprint, inventory revision or fixture value `1`.

The requirement/architecture decision for this foundation is
`READY_FOR_WORKFLOW`: explicitly registered private operator sidecars under
`cg.connector-capability` provide complete scoped reference aliases. Each alias
retains its operator-selected revision. The producer join must equal the loaded
alias pin in every field; its capability ID, descriptor ID and native payload ID
must agree. The payload must parse as the existing native CapabilityDefinition
and canonically equal the actual current resolved ActionStep definition. A stale
registry basis still denies. These aliases name authenticated current data;
they neither grant effects nor replace native CG-09, Process/Context, exact
consent, inventory and argument-schema checks.

The budget agent independently reviewed this assembly as implementable, Root
accepted its ownership boundary, and the gate-review agent is integrating the
typed join edges into the bounded loader. This decision is a contract for the
next implementation slice, not a claim of executed model-producer qualification.


### Qualified bounded reference discovery and current-store correction

The bounded document-discovery slice now passes 11 focused tests; the separate
current native capability-alias check passes two. The loader follows native
SemanticTaskIR reference bindings, native OutputContract schema references,
native VerificationContract rule/verifier references and the typed producer
join, including capability aliases. Every edge must match the entire registered
pin. Missing references, same-ID conflicting revisions, unsupported versions,
unused documents and bounded expansion are denied. Only ordinary files are
read; directory and FIFO denial are tested. Checked owned bytes are retained.
Context-analysis, projection and source-metadata records remain explicit opaque
leaves pending their real owning adapters; there is no claim of a complete
executable producer from document discovery alone.

Root reviewed the loader, the gate-review agent independently reviewed the
Root-authored capability-alias check, and the budget agent supplied a test-only
fixture that actually runs native planning, resolution, current policy and
ContextCompilation for two planned inspection steps. Its positive/denied paths
pass one focused test, without model inference or effects. The exact qualified
sources and logs are retained in
[the reference follow-up archive](EPIC-08.03-runtime-reference-followup-validation.json.gz).
The next positive RuntimeAdmission integration uses that real fixture; it is
not retroactively covered by these focused tests.

Independent verifier review additionally found that comparing a store locator
path cannot detect changed contents at that same path. The product final guard
now loads the actual currently selected repository claim and compares the full
initial FencedSession. Its PostgreSQL regression proves that an unchanged
admission and still-readable original handle cannot qualify a changed locator;
restoring the locator accepts, and a real cancellation append rejects. The
focused test passed and is retained in
[the current-store regression archive](EPIC-08.03-goal-current-store-validation.json.gz).
The reviewer cleared this correction. The last completed package Clippy run,
before the selector-association module was installed, failed on the uninstalled
TargetFactory interfaces. The subsequent selector module is still being
implemented and has no current package-wide lint qualification. Full expanded
coverage and system acceptance remain open. No lint suppression or replacement
consumer was added.


### Same-step observation authority refinement

The budget agent independently traced the apparent session-step versus read
policy-step conflict. No new authorization-step protocol is required: native
`planner::plan`/`make_step` can group mandatory requirements sharing the same
originating DeltaItem, and native selected Skill closure can include both
Inspect and Mutate requirements with explicit real providers.
`resolution_application::inspect_resolution` retains that closure;
`PolicyApplication::step_input` supplies its actual current definitions.
Existing source tests include
`adds_information_and_verification_dependencies_when_explicitly_required`,
`transitive_mutation_requirements_need_explicit_providers_and_detect_cross_cycles`
and `capabilities_added_by_skill_closure_cannot_escape_policy`. This turn
inspected these existing component paths; it did not execute a live mutation
workflow or claim those tests as a new run.

Decision: `READY_FOR_WORKFLOW` for SourceFactory admission requiring each
Snapshot/PostEffect/Goal Readcap in the actual ActionStep closure. The actual
operator-supplied workflow/catalog and current resolution basis must establish
that dependency; missing Readcap means zero connector I/O. A separate
verification-step requirement does not establish it. Root accepted this native
assembly and retains exact request.step binding, actual CG-09 and consent.

### Native model transport refinement, 2026-10-26

Root traced the actual Chat Completions adapter; the budget agent independently
reviewed requirements, architecture and evidence. The provider's
[request reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)
restricts function names to 64 ASCII letters, digits, underscores or hyphens.
The [function-calling contract](https://developers.openai.com/api/docs/guides/function-calling)
requires every object property to be required for strict decoding. Dotted native
capability IDs and optional native arguments require an explicit adapter profile.
Capability identities and admitted argument schemas remain authoritative.

Decision: `READY_FOR_WORKFLOW` for `PortableV1`, with templates 3/4 distinct
from unchanged legacy templates 1/2. Each 64-character alias binds the complete
admitted ToolDefinition, including capability, inventory and schema pins. The
invocation-local inverse rejects unknown names and native-ID fallbacks; any
collision rejects the table. Optional native scalar arguments retain their exact
schema and use provider `strict:false`; gateway argument validation is mandatory.
`ToolProposalTextV1` preserves empty text only alongside actual calls and only
for an admitted String output. No model text or proposal proves an effect or goal.

| ID | Requirement/source | Production path/owner | Dependencies and actual state | Implementation evidence | Verification and evidence level | Status |
| --- | --- | --- | --- | --- | --- | --- |
| MT-1 | Provider-compatible names/schema | model_adapters.rs / Root | Explicit profile and actual admitted toolset | Portable aliases, exact inverse, unchanged schemas, templates 3/4 | Real loopback HTTP checks emitted wire and canonical proposal verification; COMPONENT | VERIFIED |
| MT-2 | No credential reflection into retained output | model_adapters.rs / Root | Runtime credential and actual JSON decoding boundaries | Checks decoded response, arguments and result; result-only path retains original bytes for strict parsing | Escaped content and nested arguments/results denied in both Chat adapter kinds; COMPONENT | VERIFIED |
| MT-3 | Modes and template bound to current producer | Native ModelProducerFactory / Gate | Owning DTO implemented; factory not installed | Typed explicit response/wire modes and exact pins; consumer pending | Transport mismatch denies before HTTP; actual native producer join pending | PARTIAL |
| MT-4 | Complete issue/parent acceptance | LocalConnectorRuntime and CG14 | Native composition, consent lifecycle, installed/live and full gates pending | Preparation/execution still unsupported | Component results cannot establish complete acceptance | OPEN |

The suites pass 38 and 11 tests respectively, with one existing external-provider
test ignored. Exact sources, logs and limitations are retained in
[the transport archive](EPIC-08.03-portable-model-transport-validation.json.gz).
Only the supported closed object/scalar connector input profile was qualified;
nested connector argument schemas remain unsupported. No external model call,
installed client, complete consent lifecycle or full coverage is claimed.

### Current data ownership and context source refinement

The factory requires actual normalized observations rather than a static fragment
declaring its own quality. Captured source material owns the complete actual read
payload, request and full session/producer binding in memory. `available` means
those bytes are present; it does not claim PostgreSQL retention. The coordinator
must persist the complete immutable Before snapshot at its legal checkpoint
before effects, without changing a live preparation allocation's claim revision.

Native Plan/Situation basis artifacts use actual native bodies and IDs plus the
existing `content_reference(id, bytes)` owner's returned revision and digest.
They cannot borrow the CLI wrapper's ID or guess a revision from another target.
This dynamic artifact owner also supports replanning; mandatory static operator
aliases would require rewriting configuration for every new native plan. Root
and Gate independently reviewed this decision. Captured artifacts remain data,
and the native factory must validate the actual current Source and exact bytes.

Root implemented and Gate independently reviewed the native source-class fix:
SystemState and StructuredData target requirements can select `ToolsMcp` while
preserving ObservedEvidence. StructuredData accepts a matching Evidence fragment;
DomainKnowledge remains Knowledge-only. Semantic state/data still consumes its
Knowledge reservation, with no borrowing from Evidence capacity. The
[native context archive](EPIC-08.03-observed-mcp-context-validation.json.gz)
retains 21 application and 27 context passing tests, including Artifact/Entity
handoffs and revision, quality, staleness, authority and budget negatives. This
is component evidence with injected metadata, not actual Source acquisition or a
complete native producer qualification.

The base resource check authenticates fresh registration, actual selected store,
current scoped claim, limits, fencing and deadlines. It supplies no operation
right. Full Source/Model/Goal checks still load all current definitions. Gate's
architecture and Phase's requirements/test review accepted separating these
lookups, avoiding repeated producer loads in a resource-only check without
caching authority. The [resource boundary archive](EPIC-08.03-resource-only-boundary-validation.json.gz)
retains 15 actual local-admission tests. That initial Source/stdio attempt
exceeded the unchanged claim lease; subsequent component qualification below
supersedes this timing result. Runtime acceptance remains OPEN.

### Fresh connection and historical producer boundaries, 2026-10-26

Root implemented a private connection slot within one LocalSessionAccess.
Gate's architecture review and Phase's requirements/test review independently
accepted the design. Every lookup rereads the actual bounded StoreDefinition,
checks its exact value and canonical scope, then performs fresh PostgreSQL
queries. Changed definitions evict the old connection before reconnecting;
failed operations are not retried. No checkpoint, consent or policy result is
cached. The [connection archive](EPIC-08.03-connection-reuse-validation.json.gz)
retains 15 local-access, 21 session-store and one complete goal-verifier test.

The subsequent actual stdio Source/Target component completed within the original
30-second lease, including a separate scoped CognitiveStore data write/read,
native normalization and a receipt for the complete original goal. This is not
the session ConnectorObservation codec or a completed mutating session. The
native model component separately passed 14 tests through actual source capture,
context compilation and loopback HTTP tool proposals. Its factory is now changing
for consent integration; this historical component result does not qualify the
new bytes or the installed runtime.

Gate and Budget independently reviewed the historical-producer lifecycle.
Decision: READY_FOR_WORKFLOW for an opaque data-only RetainedInvocationIr and a
separate pure retained-consent preflight. Retained data must reconstruct through
native structural checks and equal the complete authenticated original record
and native identity. It cannot be passed to model, verifier or effect APIs as an
admitted invocation. Promotion must use existing current semantic-reference and
admission checks after the budgeted fresh approved-resume read establishes
applicability. Both approval and continuation must recheck the exact live grant
and expiry after preflight; legacy strict revalidation remains the default.

The [retained IR archive](EPIC-08.03-retained-ir-data-validation.json.gz) retains
eight passing tests, including stale-current and invalid-reference promotion
denials and full original-record, identity, version and structural mismatch
denials. The PostgreSQL retained-producer owner, coordinator preflight hooks and
native approved-resume composition remain in progress. These tests establish a
data boundary, not completed durable resume or full issue acceptance.

The coordinator preflight slice now passes its complete 53-test session runtime
suite. The [source-bound preflight archive](EPIC-08.03-retained-consent-preflight-validation.json.gz)
also retains the focused four-case callback race matrix: approval/continuation
with withdrawal/expiry during preflight. No case enters runtime resume or spends
its phase allowance, and the ledger remains unchanged.

CurrentProducerDefinitions now shares the live Source's pure native assembly.
It captures fresh authenticated configuration and exact PostgreSQL checkpoint
before and after assembly, checks the original repository too, and retains full
canonical pin/body pairs. It needs no allocation or expired preparation lease;
it compiles the Inspect producer while leaving action consent facts empty and
never compiles an action or creates an operation grant. Budget independently
reviewed the data owner and found no concrete defect. The
[current definitions archive](EPIC-08.03-current-native-definitions-validation.json.gz)
retains one actual PostgreSQL test and seven complete Source regression tests.
The positive test uses a real Runnable checkpoint with no reservations or lease;
later actual consent-configuration changes produce a different policy revision.
Foreign owner, revision, step and cancellation cases deny. Historical native
reconstruction and fresh approved-resume verification remain separate work.

`BATCH-CONSENT-1` remains OPEN: preparation currently supports one distinct
consent request and rejects multiple distinct required grants before asking or
dispatching. This temporary fail-closed behavior does not reduce the accepted
batch or parent-epic scope.
