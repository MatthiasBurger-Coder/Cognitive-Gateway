# EPIC-08 child issue acceptance matrix

Snapshot: 2026-10-26, based on commit
`742c318b47b1e06ced615618fe7ac0c1d05f71ce` plus uncommitted worktree changes.
GitHub #272–#279 were fetched for this review and remain open/PLANNED. Statuses
below distinguish component evidence from complete shipped-path or live proof.

The clean final18 all-target tests passed, with an expanded Application coverage
gate pass and three Daemon coverage gaps (see the parent matrix). Later
canonical policy/audit and shared-access changes have focused tests only;
current full-candidate coverage is not accepted. Historical final15/final16
measurements below cannot qualify the newer source.

## #272 — Task session contracts, lifecycle and ownership

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Positive and invalid state transitions are covered | PASS_COMPONENT | `gateway-application/tests/session_contracts.rs`, `session_runtime.rs`; final workspace run. |
| Duplicate, stale and cross-scope commands are refused | PASS_COMPONENT | `session_contracts.rs`, `gateway-daemon/src/session_store_tests.rs`, `gateway-daemon/tests/codex_sessions.rs`. |
| Terminal results cannot be silently restarted | PASS_COMPONENT | Session state transition and restart/recovery tests in `session_contracts.rs`, `session_runtime.rs`. |
| Model and client inputs cannot grant permissions | PASS_COMPONENT | `connector_consent.rs`, `codex_contracts` and policy gate tests. |

## #273 — Trusted Rust session coordinator

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Deterministic structured task completes without models | PASS_COMPONENT | `gateway-daemon/tests/codex_sessions.rs`; structured artifact session path. Connector effects remain unsupported. |
| Every dispatch recompiles current authority | PASS_COMPONENT | Coordinator admission/revalidation tests in `gateway-application/tests/session_runtime.rs`. |
| No duplicate planner or policy semantics | PASS_COMPONENT | Coordinator delegates to current Context/Policy application boundaries; architecture gate passed. |
| Unsupported paths fail explicitly | PASS_COMPONENT | Default connector runtime returns `UnsupportedCapability`; unsupported invocation/model paths have negative tests. |
| Authenticated observations are not model-generated claims | PASS_COMPONENT | Snapshot and goal-readback contracts/tests in `connector_goal_verification.rs` and `session_runtime.rs`. |

## #274 — Proposal dispatch and verification

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Unknown tools, schema mismatch, changed inventory, policy denial and stale consent yield zero dispatch | OPEN | Component gates and negative tests exist; shipped `LocalConnectorRuntime` and production `ConnectorPolicySource` are absent. |
| Successful tool return alone cannot satisfy the goal | PARTIAL | Independent final readback and synthetic negatives exist; no installed provider/readback path. |
| Two-step workflows retain complete scoped snapshots | PARTIAL | Synthetic coordinator and two-process store tests; no live provider effect or database-server restart qualification. |
| Partial/uncertain outcomes are typed | PARTIAL | Typed outcomes and Unknown/no-redispatch recovery are tested; shipped runtime/provider reconciliation absent. |

Detailed decision and 3-Amigos review: [`EPIC-08.03-three-amigos-gate.md`](EPIC-08.03-three-amigos-gate.md).

## #275 — Clarification and consent pause/resume

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Duplicate, expired and wrong-scope replies fail | PASS_COMPONENT | Interaction revision/expiry/scope checks in session contract/runtime tests. |
| Semantic clarification never grants permission | PASS_COMPONENT | Clarification and mutation-consent paths are separate; consent denial/revalidation tests. |
| Changed action or arguments invalidate old consent | PARTIAL | Binding validation and connector consent contracts/tests exist; no production connector resume/dispatch path. |
| Denial and withdrawal are recorded | PASS_COMPONENT | Artifact and connector consent ledger/event tests. |
| Restart preserves pending interaction and consumed budgets | PASS_COMPONENT | PostgreSQL-backed shared-session recovery test; no DB-server restart qualification. |

## #276 — Durable journal, restart recovery and reconciliation

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Crashes before/after send and before result commit are tested | PARTIAL | Crash points and two-process restart are tested with controlled runtime; external provider effect and PostgreSQL server restart are unqualified. |
| Competing owners cannot both dispatch | PASS_COMPONENT | PostgreSQL fencing and competing-owner qualification tests. |
| Unknown external effects block instead of retry | PASS_COMPONENT | Recovery retains typed Unknown and does not redispatch. |
| Recovery revalidates authority and scope | PASS_COMPONENT | Recovered command/scope/fence and current authority checks are tested. |
| Corrupt or incompatible checkpoints fail closed | PASS_COMPONENT | Version, digest, identity and record corruption tests. |
| No external exactly-once guarantee is claimed | PASS_COMPONENT | Runtime and connector documentation explicitly model uncertain effects; no exactly-once promise. |

## #277 — Aggregate budgets, cancellation and no-progress

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Nested adapters cannot exceed session limits | PARTIAL | Typed cumulative resource limits and conservative whole-step reservations now persist in the fenced session journal before preparation and approved resume. V2 phase allocations and the model child run now enforce disjoint ceilings, currency and shared retry/fallback counters; a reserved connector adapter now enforces hard quotes, exact CG-09 reservation binding and live checks in controlled-peer tests. Current transport requalification, retrieval/worker enforcement and production composition remain open. |
| Unknown usage is bounded conservatively or refused | PARTIAL | Common session resource ledger retains the full admitted upper bound across missing telemetry, consent and process restart; corruption/rollback/cap tests pass. The model child run retains conservative output/cost/unknown-output reserves and qualifies known conservative token bounds; live provider behavior and other concrete nested adapters remain unqualified. |
| Cancellation blocks later dispatch and preserves uncertain in-flight effects | PASS_COMPONENT | Runtime cancellation/deadline and Unknown recovery tests. |
| Loops with no progress terminate | OPEN | 3-Amigos gate: use CG-14's strict decrease of open `actionable_items`; the coordinator lacks trusted complete per-step assessments. Contract choice and missing-assessment behavior are awaiting user resolution. |
| Multi-session admission/backpressure is bounded | PARTIAL | Local MCP transport is single-worker/zero-queue and model/worker adapters have bounds; global multi-session admission is not proven. |

## #278 — CLI, packaging and observability

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Fresh scoped installation runs the structured-task walkthrough | PARTIAL | Checked-in walkthrough and CLI/MCP component tests; no fresh-install operator run evidence. |
| Health differs from qualified capability readiness | PASS_COMPONENT | `local_mcp/runtime.rs` now reports `readiness_scope=transport_only` and `capability_readiness.status=unqualified`; focused test passed and runtime module coverage is 555/577 (96.19%). In the final16 all-target daemon report, the broader `local_mcp/mod.rs` gate passes at 361/377 (95.76%). |
| Traces correlate task, step, model, tool and evidence without secret payloads | PARTIAL | Scope/correlation metadata and sanitized diagnostics exist; complete cross-layer trace correlation is not implemented/qualified. |
| Restart/shutdown and database recovery are demonstrated | PARTIAL | Process-level PostgreSQL journal recovery is tested; server restart, shutdown runbook and live effect reconciliation are not qualified. |
| Upgrades preserve version and authority checks | PASS_COMPONENT | Versioned schema/checkpoint compatibility and authority revalidation tests. |

## #279 — Complete live-system qualification

| Acceptance criterion | Status | Evidence / remaining gap |
|---|---|---|
| Policy denial, injection, cross-scope, malformed output, outage, cancellation, budget exhaustion, stale consent, uncertain mutation and crashes are tested | PARTIAL | Broad component adversarial coverage exists; no single integrated live-session qualification covers the full matrix. |
| Deterministic replay never invokes external actions | PASS_COMPONENT | Replay/recovery tests use no-dispatch paths and assert no redispatch. |
| Live and synthetic evidence are labelled | PASS_COMPONENT | EPIC-08.03 evidence separates synthetic component and process-backed store results; no live provider result is claimed. |
| Pinned versions/digests and all applicable >=95% gates are required | PARTIAL | Final15 Application plus final16 Daemon session coverage passes all selected >=95% gates; `local_mcp/mod.rs` is 361/377 (95.76%), `codex_workspace.rs` 451/456 (98.90%), and `cg-local.rs` 117/122 (95.90%). No immutable release candidate or complete system evidence set is qualified. |

## Release decision

The matrix shows implemented contracts and passing component tests, but it does
not accept #271 or close any child issue. Full acceptance still requires the
installed connector/model path, cumulative session budgets, trusted per-step
CG-14 progress evidence, complete trace correlation, operational qualification,
and controlled live effects. The user-selected “existing MCP servers” source
does not yet identify a product outbound server or target configuration.
