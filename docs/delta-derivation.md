# Deterministic Delta Derivation v1 (CG-07.03)

CG-07.03 projects a complete CG-07.02 comparison tree into the existing
provider-independent `Delta` IR. The projection is pure: it reads only the
explicit `DesiredState`, `CurrentState`, optional `Situation`, comparison
rules and derivation rules. It never grants permission to act and never
resolves capabilities, Agents, Skills, Processes or policy.

## Classification

Each required leaf condition becomes one `DeltaItem`. The `DeltaKind` is the
action-independent gap category and `DeltaReasonCode` retains the detailed
machine-readable reason from comparison. The rationale and
`RequiredOutcome` description are generated from that same classification.

| Comparison outcome | Delta kind | Required outcome |
| --- | --- | --- |
| `SATISFIED` | `SATISFIED` | `NO_OP` |
| `UNSATISFIED` for a positive condition | `UNSATISFIED_CONDITION` | `DOMAIN_CHANGE` |
| An explicit restricted value is present, or a negated expression is false | `VIOLATION` | `DOMAIN_CHANGE` |
| `UNKNOWN` because the subject/state is not established | `UNKNOWN_STATE` | `OBSERVATION` |
| `INSUFFICIENT_EVIDENCE` | `MISSING_EVIDENCE` | `EVIDENCE_ACQUISITION` |
| `CONFLICTED` | `CONFLICT` | `CONFLICT_RESOLUTION` |
| `UNRESOLVED_INPUT` | `UNRESOLVED_INPUT` | `INPUT_ACQUISITION` |
| `INCOMPARABLE` | `UNSUPPORTED_COMPARISON` | `ASSESSMENT` |

Unknown information is never converted into a factual violation. A missing
subject therefore produces `UNKNOWN_STATE`; explicitly unsupported evidence
produces `MISSING_EVIDENCE`; conflicting assertions remain `CONFLICT`.

## Logical expressions and actionable items

The full comparison tree is retained by `DeltaDerivation::comparison()`. Leaf
items are emitted only for branches required by the expression:

- `ALL` requires every branch;
- `ANY` requires every branch until one branch is satisfied, after which only
  the satisfied branch is retained as a non-actionable explanation item;
- `NOT` reverses only definite `SATISFIED` and `UNSATISFIED` outcomes. A
  satisfied child under `NOT` becomes an explicit `VIOLATION`.

By default, satisfied required conditions remain in the Delta as
non-actionable `SATISFIED` items so their basis and rationale are available.
`DeltaDerivationRules::with_satisfied_explanations(false)` omits those items;
the comparison tree remains available through `derive_delta_with_rules`.
`Delta::actionable_items()` filters the non-actionable items, and
`Delta::is_noop()` is true when no actionable item remains.

## Identity, ordering and traceability

Delta item identities use the canonical expression path (`condition.0`,
`condition.0.1`, …). DesiredState canonicalization makes equivalent
commutative expressions use the same paths. Repeated equivalent leaf
conditions are deduplicated by condition, classification, reason and
requiredness. The Delta aggregate then sorts items by typed identity.

Every item retains:

- its owning `DesiredState` and originating `ConditionId`;
- `SituationId` and `CurrentStateId`, when supplied;
- compared subjects and normalized fact, observation, evidence and provenance
  references;
- relevant Situation assessment and diagnostic references;
- an explicit typed `RequiredOutcome`.

Only references are copied into `DeltaBasis`; raw evidence and sensitive
payloads are never embedded in Delta rationale or diagnostics. The full
comparison tree is available separately when a caller needs branch-level
explanation, including branches intentionally omitted from the actionable
Delta.

## Boundary invariants

The derivation validates all supported v1 versions and rejects a Situation
whose observed-state identity differs from the supplied CurrentState. It
performs no capability, Agent, Skill, Process or policy resolution. A Delta
describes a gap and a desired outcome, not authorization or an execution
command.
