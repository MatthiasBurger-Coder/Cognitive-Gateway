# Declarative Planning IR v1 (CG-07.01)

CG-07 owns the provider-independent semantic chain from a desired state and
current situation to a declarative plan:

```text
DesiredState + Situation
        ↓
Delta
        ↓
CapabilityRequirement[]
        ↓
Plan / PlanStep[]
```

The v1 domain contracts are implemented in `gateway-domain::planning`, with
the comparison, Delta, information-input and capability-requirement derivation
layers in their dedicated modules. They define the provider-independent data
boundary; they do not select concrete executors or execute a plan. See
[`capability-requirements.md`](capability-requirements.md) for CG-07.05.

## Ownership

| Concern | Owner | CG-07 boundary |
| --- | --- | --- |
| `DesiredState`, `CurrentState`, `Situation`, observations and evidence | CG-06 | CG-07 references their typed identities and preserves lineage references. |
| `Delta`, `DeltaItem`, required outcomes | CG-07 | Describes gaps and the outcome needed to close them; it does not grant authority. |
| `CapabilityRequirement` | CG-07 | References an abstract canonical `CapabilityId`; it never selects an Agent or Skill. |
| `Plan`, `PlanStep` and declarative dependencies | CG-07 | Describes required outcomes and verification shape; it is not a process instance. |
| concrete Agent/Skill/ProcessDefinition resolution | CG-08 | Consumes the abstract requirements and Plan later. |
| lifecycle legality and transitions | CG-04 | A Plan lifecycle hint never replaces a process definition or transition. |
| authorization and policy decisions | CG-09 | A requirement or Plan does not authorize mutation. |
| compiled context and `ExecutionContextIR` projection | CG-10 | Plans remain upstream input. |

## Version and deterministic identity rules

`PlanningIrVersion` is a distinct `MAJOR.MINOR` value object. Only `1.0` is
currently supported; syntactically valid future versions are rejected at
the aggregate boundary. `DeltaId`, `DeltaItemId`, `CapabilityRequirementId`,
`PlanId` and `PlanStepId` are validated typed identifiers.

Collections are sorted by their typed identity and duplicate identities or
references fail closed. Delta items must reference their owning
`DesiredState`. A Plan validates capability-requirement references internally
and can validate Delta-item references through
`Plan::validate_against_delta`. Plan-step self-dependencies are rejected;
complete DAG and deterministic topological-order semantics are implemented in
CG-07.06. `Plan::topological_order()` exposes dependency-first order,
`Plan::parallel_layers()` exposes explicit independent antichains and
`PlanStep::prerequisites()` keeps prerequisite conditions separate from graph
edges. Cycles, self-dependencies and dangling edges fail closed at Plan
construction.

## Semantic records

`DeltaItem` identifies the originating desired condition, its `DeltaKind`, a
`DeltaBasis` and a typed `RequiredOutcome`. `DeltaBasis` can retain
`Situation`, `CurrentState`, state-subject, fact, observation, evidence,
provenance and assessment identities without embedding raw evidence.

`CapabilityRequirement` carries a canonical abstract capability identity,
mandatory/optional cardinality, capability-contract preconditions and
constraints, the originating Delta item and rationale. CG-07.05 derives these
requirements only from explicit outcome bindings and validates the canonical
contract's abstract safety class. It has no fields for Agents, Skills,
ProcessDefinitions, providers or runtime handles.

`PlanStep` carries a typed outcome, dependency references, capability
requirement references, Delta trace references, a completion condition and an
optional separate verification condition. `LifecycleRequirement` expresses
generic shape such as `VERIFICATION_AFTER_CHANGE` or `EVIDENCE_BEFORE_CHANGE`;
it is a declarative hint and never selects a concrete process.

An empty Plan is a valid no-op. An explicit `NO_OP` step is also supported for
callers that need a traceable no-op node. Deterministic comparison, Delta
classification, information-resolution inputs and abstract capability
requirements are implemented in the CG-07.02 through CG-07.05 modules;
CG-07.06 adds graph validation and ordering, and CG-07.07 adds the deterministic
rule-based planner. See [`deterministic-planner.md`](deterministic-planner.md)
for the planner's generic outcome, dependency and diagnostic semantics.
