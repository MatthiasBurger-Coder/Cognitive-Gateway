# EPIC-08 #271 — Parent acceptance matrix

Snapshot: 2026-10-26, based on commit `742c318b47b1e06ced615618fe7ac0c1d05f71ce`
plus uncommitted worktree changes. This is an audit snapshot, not immutable
release evidence. Source of truth: GitHub #271 and child issues #272–#279,
current worktree, workspace tests, separated Application/Daemon all-target
coverage reports from the final16 daemon and final15 application qualification runs, and the EPIC-08.03 3-Amigos review. “Component evidence” does
not establish shipped-path or live-provider qualification.

Qualification update: the clean final18 Application and Daemon all-target
tests passed. The expanded Application coverage gate passed; the Daemon gate
did not: `local_sessions.rs` 1673/1774 (94.31%), `mcp_client/mod.rs` 104/116
(89.66%), and `session_budget.rs` 351/370 (94.86%). Source-bound final18
reports/logs are archived in the #274 evidence record. Subsequent canonical
step/audit-envelope and shared-access refinements have focused test evidence;
their full current-candidate coverage remains unqualified. Older measurements
below retain their historical scope and cannot accept these changes.

Detailed criteria for each child issue are recorded in
[`EPIC-08-child-acceptance.md`](EPIC-08-child-acceptance.md).

| #271 acceptance criterion | Status | Evidence present | Remaining proof / gap |
|---|---|---|---|
| Scoped structured task completes through the composed runtime without an LLM | PARTIAL | `crates/gateway-application/tests/session_runtime.rs`; `crates/gateway-daemon/tests/codex_sessions.rs`; PostgreSQL-backed shared-session path | Qualify a fresh scoped installation from documented setup through final verified result. Connector goal execution is not installed. |
| Supported natural-language task completes through EPIC-05 semantics when available | OPEN | EPIC-05 semantic compilation and clarification components exist independently | No session-host integration proof showing natural-language input entering EPIC-05 and reaching the shared session lifecycle. |
| Ambiguity pauses for minimal clarification; reply re-enters semantic validation | PARTIAL | Session interaction contracts and clarification/resume component tests | Prove the complete EPIC-05-to-session path with a supported natural-language task and current semantic validation after reply. |
| Consent binds exact action/scope/version and is revalidated before dispatch | PARTIAL | Artifact consent path; connector consent binding, ledger, withdrawal and denial tests in `connector_consent.rs` and `session_runtime.rs` | Connector runtime is not installed; prove current CG-09 policy and consent immediately before a real provider dispatch. |
| Models cannot directly invoke tools, change policy or manufacture observed success | PARTIAL | Invocation IR/output verification and policy-gate tests; provider output is a proposal | End-to-end session invocation-to-connector qualification is absent; verify against installed composition and a controlled target. |
| Every action crosses existing Process/Policy/Context boundaries | PARTIAL | Existing compilation and session artifact path; component policy/admission contracts | No shipped connector runtime path to demonstrate all three boundaries for each external action. |
| Authenticated post-action observations prove the complete goal and acceptance criteria | PARTIAL | Independent scoped snapshots/readback contracts, typed validators and synthetic two-step coordinator coverage | No installed live readback source or controlled external effect. |
| Session recovery preserves consumed budgets, revisions and dispatch identity | PARTIAL | Fenced journal and process-restart recovery tests; actions/retries/deadline and typed conservative resource reservations persist | V2 phase/model allocations and a reserved connector adapter are implemented; current connector transport requalification is in progress, while retrieval/worker enforcement and full production composition remain absent; database-server restart and live-provider recovery remain unqualified. |
| Crash after external side effect never causes blind duplicate dispatch; unsupported reconciliation is blocked/unknown | PARTIAL | Durable journal and recovery tests retain Unknown and do not redispatch | Qualify with a live controlled provider effect and operator reconciliation/readback protocol. |
| Fenced ownership prevents competing coordinators dispatching the same step | PASS_COMPONENT | PostgreSQL journal fencing and competing-owner tests in `session_store_tests.rs` / `codex_sessions.rs` | Retain the test run as release evidence and qualify with the deployed database configuration. |
| Cancellation, timeout, no-progress and overload are bounded and observable | PARTIAL | Adapter-level cancellation/deadline/backpressure tests and typed session cancellation states | Session-wide no-progress limit and overload admission bounds are not established; provide correlated operational metrics for these outcomes. |
| Final output distinguishes verified completion, partial work, blocked state, cancellation and failure | PASS_COMPONENT | Typed session/result states and contract/runtime tests | Verify the same distinctions through every shipped CLI, Codex and service projection. |
| CLI, Codex and service adapters reuse one session application boundary | PARTIAL | Shared coordinator and Codex/local adapter tests | Complete adapter parity and a shipped service adapter are not qualified. |
| No secret, hidden chain-of-thought or cross-project payload leaks into traces/results | PARTIAL | Credential scanning, sanitized diagnostics, project isolation and boundary tests | Full task/step/model/tool/evidence trace correlation and end-to-end secret-leak qualification are missing. |
| Linux/container operation, restart, backup/restore and troubleshooting are documented and qualified | OPEN | Operational/session documentation and PostgreSQL recovery implementation exist; MCP health now reports transport-only readiness separately from unqualified capability readiness, with a focused runtime assertion | Fresh installation, container lifecycle, graceful shutdown, backup/restore, database restart and operator recovery run evidence are not complete. |
| Every acceptance criterion maps to automated evidence and established architecture/coverage gates | PARTIAL | This matrix; architecture/contract checks; workspace tests; separated all-target coverage; 3-Amigos #274 report | Final15 Application plus final16 Daemon session coverage gates pass; `local_mcp/mod.rs` is 361/377 (95.76%) and `codex_workspace.rs` is 451/456 (98.90%). System-level live evidence is also missing. |

## Release decision

Parent #271 is **not accepted**. The current expanded Daemon coverage gate remains open as recorded above. Historical final15 Application and final16 Daemon reports passed their then-selected gates; `local_mcp/mod.rs` is 361/377 (95.76%) and `codex_workspace.rs` is 451/456 (98.90%). Evidence still lacks the installed connector runtime, production composition of connector resource enforcement, retrieval/worker enforcement of cumulative caps, session-wide no-progress and overload limits, end-to-end observability, and controlled live effects.
Issue #274 must remain open until its own shipped-path criteria pass; #271 must
remain open until every parent criterion, including #279 live qualification,
has revision-bound evidence.

## External qualification prerequisite

The configured connector runtime and current authority adapters are still
repository implementation work. The new `SessionResourceLedger` now persists
immutable admission and conservative cumulative model/token/cost/retrieval/
repair/worker reservations alongside `SessionBudget`'s action/retry/deadline
baseline. The coordinator reserves before resource-consuming preparation and
claims the same allowance before approved-proposal reconstruction. Failed
persistence releases no runtime call; interruption, cancellation and consent
expiry retain the charge without automatic preparation replay. Separate
PostgreSQL writer/reader processes prove retained consumption and rejection of
refill/removal. V2 phase allocations and the model child run now enforce their disjoint bounds,
producer/claim identity and shared retry/fallback counters. The reserved connector adapter now enforces hard quotes and exact CG-09 reservation
bindings in controlled-peer tests; fresh transport/all-target requalification is
in progress. Retrieval/worker enforcement and production CG-09 reservation
lookup remain required. The standalone invocation
journal cannot replace the session's fenced journal for this purpose.

The requested source choice is “existing MCP servers”. Local discovery found
no product outbound MCP server entry or operator target configuration. A
specific server/configuration path or endpoint is still needed, along with its
target/manifest/mapping, credential mechanism, current CG-09 policy and consent
source, aggregate budget reservation, and independent snapshot/readback
protocol. Until an operator-provided MCP configuration is available, the local
connector runtime remains fail-closed and no live effect is claimed.
