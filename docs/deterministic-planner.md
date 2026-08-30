# Deterministic Declarative Planner (CG-07.07)

`gateway-domain::planner` transforms a validated `Delta` and abstract
`CapabilityRequirement` values into a provider-independent `Plan`. The planner
decides required outcome nodes and explicit graph edges, but never selects an
Agent, Skill, ProcessDefinition, provider or runtime.

## Rule contract

The planner is versioned by `PlanningIrVersion` and currently exposes
`DETERMINISTIC_PLANNER_VERSION` `1.0`. Every `PlannerDecision` carries a stable
`PlannerRuleCode`, the originating `DeltaItemId` and a non-empty rationale.
`PlannerDiagnostic` carries a stable diagnostic code and is blocking whenever
the planner cannot safely construct an executable declarative plan.

Actionable Delta items map to generic PlanStep kinds as follows:

| Required outcome | PlanStep kind | Completion contract |
| --- | --- | --- |
| `DOMAIN_CHANGE` | `CHANGE` | the originating DesiredCondition is satisfied |
| `EVIDENCE_ACQUISITION` | `EVIDENCE_ACQUISITION` | the required outcome is recorded |
| `OBSERVATION` | `OBSERVATION` | the required outcome is recorded |
| `INPUT_ACQUISITION` | `INPUT_ACQUISITION` | the required outcome is recorded |
| `CONFLICT_RESOLUTION` | `CONFLICT_RESOLUTION` | the required outcome is recorded |
| `ASSESSMENT` | `VERIFICATION` | the required outcome is recorded |

Satisfied items produce an explicit `NO_OP` decision and no fabricated step.
Unsupported outcomes produce `UNSUPPORTED_DELTA_OUTCOME` and no plan.

## Dependency and verification semantics

Each generated step traces its Delta item and abstract capability requirement.
The planner preserves independent branches. With the default rule set, an
information step precedes a change only when its Delta basis overlaps the
change's condition or state subject. `PlannerRules::allowing_independent_information`
disables this optional edge.

Equivalent requirements with the same capability, cardinality, originating
Delta item, preconditions and constraints are merged deterministically by the
lexicographically smallest requirement identity. The explicit
`with_merge_equivalent_requirements(false)` rule preserves distinct identities
when a caller needs that distinction in the resulting Plan.

An explicit verification capability requirement adds one generic `VERIFICATION`
step per domain change with a dependency on that change. The verification
requirement and optional `LifecycleRequirement` remain abstract; authorization
and concrete execution are downstream concerns. Caller-supplied prerequisite
conditions are copied to generated steps and remain separate from graph
dependencies.

`plan_from_capability_derivation` and `plan_from_capabilities` compose CG-07.05
capability derivation with planning. Missing or incompatible capability
contracts are surfaced as `CAPABILITY_CONTRACT_GAP`; blocking gaps fail closed
by returning a result without a Plan.

Stable readable identities are derived from Delta identities. IDs that would
exceed the typed identifier limit use a deterministic FNV-1a suffix. The same
validated inputs and rule version therefore produce equal plans, decisions and
diagnostics without an LLM, RAG service or network dependency.
