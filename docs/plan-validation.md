# Plan Validation, Canonicalization and Explainability (CG-07.08)

CG-07.08 is the fail-closed boundary between declarative planning and the
later CG-08 resolution layer. It validates the complete semantic chain:

```text
DesiredState condition/expression
        ↓
comparison result → Delta item → RequiredOutcome
        ↓                  ↓
CapabilityRequirement → PlanStep → graph/completion/verification
        ↓
deterministic rule and version trace
```

## Validation contract

`Plan::validation_report(desired_state, delta)` returns a deterministic
`PlanningValidationReport`. Diagnostics have stable
`PlanningValidationDiagnosticCode` values, an optional typed-artifact subject
and a non-empty rationale. They are ordered by subject, code and rationale so
the same semantic input produces the same machine output. The report is valid
only when it contains no diagnostics; `Plan::validate_for_resolution` exposes
the fail-closed hand-off used before CG-08.

Validation covers supported IR versions, cross-artifact references, Delta
coverage, abstract capability requirements, completion and verification
conditions, dependency and ordering invariants, contradictory or
mutually-exclusive outcomes, and the planner's explicit unsupported-gap
diagnostics. Local constructors already reject duplicate identities, dangling
graph edges, self-dependencies and cycles; canonical deserialization routes
through those same constructors and therefore cannot bypass these checks.

The planning model intentionally has no Agent, Skill, ProcessDefinition,
provider or runtime-handle field. The strict JSON wire structs reject unknown
fields, including accidental concrete selections. Unsupported planning gaps
remain explicit diagnostics and are never converted into guessed executable
steps.

## Canonical serialization

`Delta` and `Plan` implement canonical JSON serialization through dedicated
wire structs:

```rust
let json = plan.to_json()?;
let restored = Plan::from_json(&json)?;
assert_eq!(restored, plan);
```

`to_json_pretty` is available for review output. Deserialization rejects
unknown fields, unknown enum/version values, malformed identifiers, duplicate
identities, invalid references and graph cycles. Arrays are reconstructed
through domain constructors and are consequently emitted in deterministic
identity order. `Plan::from_json` validates the Plan's local graph and
references; callers must additionally use `validation_report` with the
originating `DesiredState` and `Delta` for the cross-artifact contract.

`DeltaBasis` serializes only typed lineage references. It never embeds raw
evidence, observations or provenance content.

## Explainability

`explain_plan` creates a human-readable projection from the same validated
`PlannerResult` used for resolution. Every entry retains the complete trace:
DesiredState and condition identity, the full condition/expression,
comparison reason, Delta basis references, required outcome, capability
requirements, PlanStep identities and dependencies, completion and
verification conditions, lifecycle requirements, planner rule identities,
planner version and rationale. `PlanExplanation::to_text` renders that same
trace without exposing raw sensitive evidence.

Explainability requires a valid Plan and an inspectable planner decision for
each Delta item. A result without a Plan or without a rule trace fails closed.

See also [`declarative-planning.md`](declarative-planning.md),
[`plan-graph.md`](plan-graph.md), and
[`deterministic-planner.md`](deterministic-planner.md).
