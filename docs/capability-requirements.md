# CG-07.05 Abstract Capability Requirements

`gateway-domain::capability_requirements` derives provider-independent
`CapabilityRequirement` values from a validated `Delta`. The derivation is a
small, deterministic binding layer:

```text
DeltaItem.required_outcome
        + explicit CapabilityRequirementRules
        + canonical CapabilityDefinition snapshot
        -> CapabilityRequirement[] + diagnostics
```

The rules bind each supported outcome kind (`DomainChange`,
`EvidenceAcquisition`, `Observation`, `InputAcquisition`,
`ConflictResolution` or `Assessment`) to a canonical `CapabilityId`. A
binding is intentionally explicit; descriptions, tags, provider relationships,
catalog position and fuzzy matching never select a capability. The capability
contract is copied into the requirement's preconditions and constraints.

Domain-changing outcomes require a `MUTATE` capability. Evidence, inspection,
analysis, observation, clarification, conflict resolution and verification
outcomes require an `INSPECT` capability. A class mismatch is retained as a
blocking diagnostic and does not produce an incompatible requirement.

Equivalent alternatives may be declared explicitly on a binding. They are
emitted as `OPTIONAL` requirements in canonical order. An absent optional
alternative is diagnostic but non-blocking; an absent or incompatible primary
contract keeps `is_execution_ready()` false. No alternative is inferred from
the catalog.

The result preserves the Delta item as the requirement's originating trace and
includes DesiredState, Situation, CurrentState and source rationale in the
requirement rationale. It has no Agent, Skill, ProcessDefinition, provider or
runtime fields. CG-08 can therefore resolve candidates later without changing
the semantic output of CG-07.

Missing bindings, missing canonical contracts, incompatible classes and
outcome/Delta mismatches are explicit diagnostics. Blocking diagnostics fail
closed through `CapabilityDerivation::is_execution_ready()`. An empty Delta
produces an empty, execution-ready derivation.

The domain API accepts a canonical capability snapshot as
`&[CapabilityDefinition]`. The registry's CG-03 capability index remains a
separate read model and can be adapted at the application/adapter boundary;
the domain crate does not depend on the registry crate.
