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

The v1 domain contracts are implemented in
`gateway-domain::planning`. They define the data boundary for later
comparison and planning algorithms; they do not perform comparison, resolve
capabilities or execute a plan.

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
complete DAG and topological-order semantics are added in CG-07.06.

## Semantic records

`DeltaItem` identifies the originating desired condition, its `DeltaKind`, a
`DeltaBasis` and a typed `RequiredOutcome`. `DeltaBasis` can retain
`Situation`, `CurrentState`, state-subject, fact, observation, evidence,
provenance and assessment identities without embedding raw evidence.

`CapabilityRequirement` carries a canonical abstract capability identity,
mandatory/optional cardinality, optional capability preconditions and
constraints, the originating Delta item and rationale. It has no fields for
Agents, Skills, ProcessDefinitions, providers or runtime handles.

`PlanStep` carries a typed outcome, dependency references, capability
requirement references, Delta trace references, a completion condition and an
optional separate verification condition. `LifecycleRequirement` expresses
generic shape such as `VERIFICATION_AFTER_CHANGE` or `EVIDENCE_BEFORE_CHANGE`;
it is a declarative hint and never selects a concrete process.

An empty Plan is a valid no-op. An explicit `NO_OP` step is also supported for
callers that need a traceable no-op node. Deterministic comparison, Delta
classification and information-resolution inputs are implemented in the
CG-07.02 through CG-07.04 modules; graph cycles and planner rules remain
subsequent CG-07 concerns.
