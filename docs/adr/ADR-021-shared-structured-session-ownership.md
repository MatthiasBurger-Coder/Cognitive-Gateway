# ADR-021 — Shared structured-session ownership and interaction authority

- **Status:** Accepted (structured runtime implemented in #294; see separate qualification evidence)
- **Date:** 2026-10-10
- **Scope:** #272/#273/#275 and EPIC-04.12 #293; extends ADR-008/ADR-009/ADR-020
- **Product decision:** User selected a verified context artifact as the explicit goal on 2026-10-10

## Current delivery state — 2026-10-11

The structured runtime and v2 binding are implemented in the shipped hosts;
[shared-runtime evidence](../shared-session-implementation.md) and
[installed-client lifecycle evidence](../codex-installed-client-qualification.md)
record their respective scopes. Fresh full-parent acceptance remains required.
The context and original admission conditions below describe the decision-time
baseline, not missing services in the current delivery. #279 remains separate.

## Original context

The inbound facade reserves session operations but provides no shared session
service. A private MCP coordinator would duplicate application authority and
leave CLI, durable recovery and authenticated verification unsupported. Existing
Rust assessment/planning/resolution/context services provide the structured
baseline; semantic/model/connector paths remain unsupported.

Frozen Codex v1 has neither run/dispatch uncertainty fields nor separate pending
consent-request and verified-consent payload contracts. Reinterpreting its
reserved projections would change authority and lifecycle semantics silently.

## Decision

Adopt the normative [shared session specification](../shared-session-contract.md).
One provider-neutral application API owns start, inspect, clarify, approve,
continue and cancel, immutable principal/scope/client ownership, command replay,
revisions, pending interactions and terminal results. #272 owns typed contracts;
#273 composes existing CG-14 and canonical services. CLI and Codex are projections
of that API, with no transport-owned task lifecycle.

The first supported Intent explicitly requests a verified ExecutionContextIR
artifact for admitted pinned inputs. `ContextApplication::compile_step` supplies
the artifact; #273 must implement a trusted artifact verifier and evidence
capture before completing it. Existing desired-state/acceptance/constraint
semantics remain unchanged. Context compilation cannot satisfy an unrelated
Intent or replace authenticated observation/verification.

#275 creates structured questions through supported validators and consent
requests through current CG policy. Typed answers pass normal admission. Trusted
consent issuance/store verification binds exact owner/task/run/pending revision,
reserved dispatch, step/action, canonical arguments, basis/authority and expiry.
Approval consumes the request and advances session revision without rebinding
the grant; Continue revalidates the unchanged binding and live revocation.
Client/model claims and Codex approvals never become CG grants.

Preserve frozen v1 schemas, tools, resources and unsupported session behavior.
The enriched session boundary requires explicit major version **2.0**, separate
request/answer/consent payloads, run/state/uncertainty fields and command-outcome
inspection. #294 publishes and qualifies exact v2 artifacts/routing after shared
service gates; no executable v2 boundary is introduced by this ADR.

Disconnect cancels waiting for a call, not the task. Only an admitted Cancel
command requests cancellation; uncertain effects prevent confirmed cancellation
or completion. #276 owns versioned durable state, conditional commits, fencing
and recovery/reconciliation. CG-14 diagnostic JSON is not a checkpoint. #277
owns cumulative budgets and deadlines; reconnect/replan/restart cannot reset them.

## Original consequences and acceptance gate

This contract decision is ready for #272 implementation. Normative contract
completion and shared-service execution are separate evidence levels. Consumers
#273/#275 implement against accepted types; #294 waits for real lifecycle,
interaction, verification, recovery and budget gates. The linked requirement
matrix records absent service evidence as NOT_RUN, never PASS.

Session operations remain unsupported until the composition root and application
services qualify. Installed-client canonical checks do not satisfy session
qualification or close EPIC-04. No storage migration or runtime rollback is
needed for this specification; rollback retains unsupported session operations.

## Alternatives

An adapter-local coordinator is rejected because it duplicates lifecycle and
policy ownership. A one-shot compiled context is insufficient verification for
a task session. Executing a different domain desired state would require its
concrete trusted runtime/observation adapter and is outside the selected baseline.
Injected session projections remain contract evidence only. Silently extending
v1 is rejected because its strict shape and consent meanings are frozen.
