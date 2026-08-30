# CG-07.06 Declarative Plan Graph

`Plan` and `PlanStep` form a provider-independent graph of required outcomes.
The graph describes dependency order and completion semantics; it is not a
CG-04 process instance, an authorization decision or a CG-08 executor
selection.

Each `PlanStep` can carry:

- explicit predecessor `dependencies`;
- abstract `capability_requirements` and Delta trace references;
- declarative `prerequisites`;
- a required `completion` condition;
- an optional separate `verification` condition;
- optional generic lifecycle metadata.

Dependencies are validated as a DAG when a Plan is constructed. Self-edges,
dangling edges, duplicate steps and cycles fail closed. Vector insertion order
never determines graph order. `Plan::topological_order()` uses canonical step
identities as a deterministic tie-breaker. `Plan::parallel_layers()` returns
sorted antichains: all steps in a layer have no dependency on another step in
that layer and depend only on earlier layers. `parallelizable_steps()` exposes
the initial independent roots.

Completion and verification are intentionally distinct. A change step can
complete when its desired outcome is reached while a later verification step,
with an explicit dependency and capability requirement, establishes that the
condition is independently satisfied. The graph never infers that dependency
or collapses the two conditions implicitly.

An empty Plan and a Plan containing only explicit `NO_OP` steps are valid no-op
graphs. No-op behavior does not create a capability or executor requirement.
