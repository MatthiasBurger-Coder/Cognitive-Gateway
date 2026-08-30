# Deterministic Comparison Semantics v1 (CG-07.02)

CG-07.02 compares the typed `DesiredState` contract with a normalized
`CurrentState` without mutating either aggregate or selecting an execution
provider. The implementation is the pure `gateway-domain::comparison` module.

## Inputs and explicit rules

`compare_condition` evaluates one declared `DesiredCondition`.
`compare_desired_state` evaluates the complete finite `ConditionExpression`.
Both require a `ComparisonRules` value. Rules carry the versioned comparison
algebra and may explicitly require fresh evidence. No function reads an
ambient clock or invents freshness information.

CG-06 `TypedValue`, `DecimalValue`, `DesiredCondition`, `ObservedState` and
`NormalizedStateEntry` remain the only value/state contracts. Numeric integer
and decimal values are compared by exact scale alignment; strings, symbols,
booleans and sets are never coerced into another type. Set membership and set
equality use deterministic canonical ordering.

## Outcomes

| Outcome | Meaning |
| --- | --- |
| `SATISFIED` | The condition is definitely true. |
| `UNSATISFIED` | The condition is definitely false. |
| `UNKNOWN` | The current snapshot does not establish a value. |
| `CONFLICTED` | Current assertions or quality metadata conflict. |
| `INSUFFICIENT_EVIDENCE` | Evidence is explicitly missing, incomplete or not fresh under the supplied rules. |
| `UNRESOLVED_INPUT` | Reserved for explicit input-dependent comparison branches. |
| `INCOMPARABLE` | Types, operation or assertion polarity cannot be compared safely. |

Missing state is `UNKNOWN`, not proof of absence. `ABSENT` is satisfied only
by a future explicit current-state absence representation; a known current
value is consequently not silently treated as absent. This keeps missing
knowledge separate from a domain assertion.

## Logical propagation

The v1 algebra is deterministic and preserves every branch in the result
tree:

| Expression | Result rule, in priority order |
| --- | --- |
| `ALL` | Any `UNSATISFIED` wins; otherwise `INCOMPARABLE`, `CONFLICTED`, `INSUFFICIENT_EVIDENCE`, `UNRESOLVED_INPUT`, `UNKNOWN`; all satisfied yields `SATISFIED`. |
| `ANY` | Any `SATISFIED` wins; otherwise `INCOMPARABLE`, `CONFLICTED`, `INSUFFICIENT_EVIDENCE`, `UNRESOLVED_INPUT`, `UNKNOWN`; all unsatisfied yields `UNSATISFIED`. |
| `NOT` | Only `SATISFIED` and `UNSATISFIED` are inverted. All uncertainty, conflict, evidence and comparability outcomes remain unchanged. |

Therefore `ALL(SATISFIED, UNKNOWN)` is `UNKNOWN`,
`ANY(SATISFIED, UNKNOWN)` is `SATISFIED` with the unknown child retained, and
`NOT(UNKNOWN)` remains `UNKNOWN`.

## Traceability and boundaries

Every result retains the observed-state identity, compared subjects, desired
and current typed values, state status, assertion polarity and normalized
fact/observation/evidence/provenance references. Nested results retain each
branch, so CG-07.03 can derive Delta items without losing explainability.

Comparison is not authorization, capability resolution, planning or mutation:

- CG-06 owns desired/current value and lineage contracts.
- CG-07 owns this comparison result and propagation algebra.
- CG-08 resolves concrete capabilities and processes later.
- CG-09 owns authorization and policy decisions.
- CG-04 owns lifecycle legality and execution transitions.
