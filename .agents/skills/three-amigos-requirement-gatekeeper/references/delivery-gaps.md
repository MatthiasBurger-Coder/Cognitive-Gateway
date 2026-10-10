# Delivery-gap review for integrations

Use when a client, adapter, service facade or runtime is being integrated, or
when an epic appears complete because its children are closed.

## Inspect runtime wiring, not just interfaces

Record the actual entrypoint and composition root, the concrete host created by
it, its supported operation list, authority source and state/persistence owner.
Check each promised operation against that concrete host. Identify default
unsupported methods, placeholder projections and test-only imports.

Label evidence explicitly:

- CONTRACT: schema or protocol compatibility without product execution.
- COMPONENT: real application computation with injected dependencies.
- EXECUTABLE: shipped binaries/launcher with a synthetic client.
- INSTALLED_CLIENT: the actual client binary, exact version and initialize
  identity, real discovery and application invocation.
- SHARED_SERVICE: actual shared lifecycle/service execution, including pause,
  consent, continuation, cancellation and required reconnect/recovery.
- SYSTEM: complete desired result with independently authenticated effects and
  verification, when the epic requires it.

These are different claims, not a universal hierarchy. State what was exercised
and what was substituted. A projected `completed` status is not a verified task
completion; no-model operation does not imply that an installed client was used.

## Prerequisite ledger

For each dependency record owner, issue, inspected code, actual state, required
contract, acceptance gate and sequencing. CLOSED is insufficient when the
service is absent. OPEN is insufficient reason to block when the required
implementation is actually present and evidenced.

Use an explicit ledger, separate from issue status:

| Dependency / owner / issue | Inspected implementation and revision | Actual state | Required contract and acceptance gate | Consumer / ordering | Included effort |
| --- | --- | --- | --- | --- | --- |

Each missing prerequisite needs an implementation owner and a gate before its
consumer starts. Record cycles or unresolved ownership as refinement work.

Distinguish external prerequisites from implementation work authorized by the
user. Do not ask permission again for necessary in-scope foundations. If scope
would include a new product plane, explicitly identify the minimum shared
foundation and preserve ownership instead of building a second local runtime.

## Estimate and slice

Include actual snapshot acquisition, exact pinned-record mapping, current policy,
consent validation, durable lifecycle/reconnect needs, installed-client protocol
compatibility, negative tests and retained qualification evidence in estimates.
A scoped baseline such as "structured tasks" still needs a named task, exact
success condition and trusted observation/result source. Compilation is not
evidence that the original desired state was achieved. If a choice changes
product semantics, resolve it before implementing consumers; continue
independent slices. Capture unknowns with a bounded investigation or contract/design slice before
promising executable consumers. Child criteria may cover only part of a parent;
name the remaining acceptance slice before any child is closed.

## Failure patterns to catch

- A facade or catalog exposes resolve/compile while the shipped host only allows
  inspect: contract/component proof has been mistaken for product availability.
- Session tests return fixed running/pause/cancelled projections: bridge proof
  has been mistaken for a shared coordinator's lifecycle.
- A synthetic client calls itself `codex / 1.0`: executable proof has been
  mistaken for interoperability with the installed Codex version.
- A PR says `Closes` while its report says the issue's required lifecycle remains
  pending: a closure decision contradicts its own acceptance evidence.
- Acceptance requirements were added after the original dependency graph was
  written: re-extract parent and child matrices and update the closure boundary.

Before claiming DONE require evidence for the exact declared scope. If a
component-only report passes, close only a component-only issue with that
explicit scope; preserve any broader issue/epic as INCOMPLETE.
