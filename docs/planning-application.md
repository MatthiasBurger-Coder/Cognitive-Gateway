# Declarative Planning Application API (CG-07.09)

`gateway_application::DeclarativePlanningApplication` is the stateless
application boundary that composes the completed CG-07 domain contracts for
CG-08 and later adapters. Every semantic input is explicit; there is no
project-global planning state and no implicit catalog lookup.

## Operation chain

The facade exposes the complete provider-neutral sequence:

```text
compare_desired_to_situation
        ↓
derive_delta
        ↓
derive_capability_requirements
        ↓
build_plan
        ↓
validate_plan → explain_plan → serialize_plan
```

The comparison and Delta methods consume the CG-06 `DesiredState`, normalized
`CurrentState` and optional `Situation` snapshot directly. A supplied
Situation is checked only for matching observed-state identity; the facade
does not rebuild or mutate CG-06 assessments, evidence or provenance.

## Capability and rule snapshots

`PlanningCapabilitySnapshot` accepts an already validated CG-03
`CapabilityIndex` together with an explicit snapshot identity and supported
planning version. The application projects only `CapabilityDefinition`
contracts into CG-07. It never queries or returns Agent/Skill provider
candidates and never performs concrete resolution.

`PlanningRuleSnapshot` records the comparison, Delta, capability-requirement
and planner rule versions used by a run. `PlanningExplainability` combines
that metadata with the domain `PlanExplanation`, so reproduction can identify
the abstract capability and rule basis without introducing ambient state.

## Output and ownership boundary

The API returns only `ComparisonResult`, `DeltaDerivation`, abstract
`CapabilityRequirementDerivation`, `PlannerResult`, validation reports,
canonical Plan JSON and explainability. It does not return or mutate
ProcessDefinitions, Agents, Skills, PolicyDecisions, process transitions or
`ExecutionContext` values. Missing capability contracts and unsupported
planner outcomes remain explicit diagnostics; they produce no fabricated Plan
and cannot be explained as executable work.

Application failures use stable `PlanningApplicationError` codes. Domain
validation, capability-snapshot integrity and canonical serialization errors
remain distinguishable at the adapter boundary.

See [`plan-validation.md`](plan-validation.md) for the domain validation,
serialization and explainability contracts.
