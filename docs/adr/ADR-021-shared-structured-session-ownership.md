# ADR-021 — Shared structured-session ownership

- **Status:** Proposed; concrete first task/result boundary under clarification
- **Date:** 2026-10-10
- **Scope:** #272/#273/#275 and EPIC-04.12 #293; extends ADR-008/ADR-009/ADR-020

## Context

The inbound facade has session contracts, but its current test projections do
not provide shared session services. A private MCP coordinator would duplicate
application authority and still leave CLI, durable recovery and authenticated
verification unsupported. The user selected a structured-task first path using
existing Rust services, with Semantic/model/connector paths unsupported.

## Proposed decision

Use one provider-neutral application session service, composing existing CG-14
and canonical application services. Outer adapters authenticate/capture inputs,
invoke supported runtimes and implement journal storage. MCP and CLI translate
commands and project results; they do not own lifecycle or consent semantics.
The [contract refinement](../shared-session-contract.md) defines ownership,
revision/replay rules, exact interaction bindings, disconnect/cancel behavior,
durable journal responsibilities and required verification.

Before implementing the consumer, fix the exact supported structured task and
its trusted result source. Context compilation alone cannot satisfy a different
Intent's desired state. A context-artifact task can complete only if that is its
explicit goal and authenticated artifact verification establishes it. A domain
execution task requires its concrete invocation/observation adapter.

Do not use CG-14 diagnostic JSON as an executable checkpoint. Durable recovery
must preserve all validated application state, pending interactions, command
ledger and consumed budgets. Unconfirmed dispatches require outcome inspection
and cannot be replayed automatically.

## Consequences and acceptance gate

The minimum shared foundations are part of the estimated delivery work, rather
than assumed prerequisites. Session operations remain explicitly unsupported
until the real application/journal services and shipped composition root qualify.
The first productive task/result decision and executable shared contract review
are required before accepting this ADR. Passing canonical host and installed
client checks does not satisfy that gate or close EPIC-04.

## Alternatives

Adapter-local lifecycle is rejected because it creates a competing coordinator.
Injected lifecycle projections remain contract tests; they cannot deliver the
shared runtime. Full semantic/model/connector implementation is excluded from
the user-selected first scope, and its owner epics remain open.
