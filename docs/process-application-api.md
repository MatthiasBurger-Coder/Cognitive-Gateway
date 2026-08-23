# Process Application API

`gateway-process` exposes `ProcessApplication` as the stable Rust application
boundary around the deterministic process core. The service is stateless: the
registry remains the single catalog model, `TransitionEvaluator` remains pure,
and `AtomicProcessMutation` remains the only authoritative transition commit
port.

## Port responsibilities

The application boundary provides these operations without leaking parser,
database or provider types:

- compile and validate process definitions;
- list, get and resolve definitions through the existing `ProcessRegistry`;
- start and inspect pinned `ProcessInstance` snapshots;
- evaluate events and commit accepted decisions separately;
- apply an event through evaluate-then-atomic-commit;
- simulate using the same evaluator without persistence;
- record evidence/blockers and invoke lifecycle pause, resume and retry
  contracts on an explicit instance snapshot;
- expose compilation and runtime explanations in machine-readable JSON and
  human-readable text.

`apply_event_atomically` returns a rejected `TransitionDecision` as a value,
while storage and atomicity failures are returned as `ApplicationError`. A
commit verifies occurrence identity, the matched transition's event type,
definition digest and expected revision through the existing mutation port.

## Simulation and explanation

Simulation is explicitly marked hypothetical and does not receive a mutation
port. Its decision is produced by the same pure evaluator used by real event
application. Runtime explanations include definition identity, instance and
occurrence identity, state projection, guard and constraint traces, stable
reason codes and the capability-first `AuthorizedActivity` projection.

Compilation explanations retain source line/column, recognized construct and
generated target identifiers. Both explanation forms have deterministic JSON
serialization; no Agent, Skill, tool, LLM, RAG or external workflow runtime is
called by this API.

## Mutation boundary

`ProcessApplication` does not own authoritative process state. Callers supply
an explicit instance snapshot and a type implementing `AtomicProcessMutation`.
Consequently, evaluation and simulation are non-mutating, while authoritative
state/history/consumed-occurrence updates remain behind one compare-and-swap
commit. Evidence and blocker helpers operate on caller-owned snapshots and
require the pinned definition's declared contracts.
