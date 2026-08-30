# Information-Resolution Planning Inputs v1 (CG-07.04)

CG-07.04 derives declarative `PlanningInput` values from a CG-07.03 `Delta`.
These values describe information that must become available before a goal
can be decided safely. They do not execute retrieval, ask a caller, choose a
source, fabricate an answer or mutate the CurrentState/Situation.

## Mapping

Only information gaps produce planning inputs. Domain-change gaps remain
Delta items for later planning and do not become information requirements.

| Delta kind | Planning input kind | Completion | Verification |
| --- | --- | --- | --- |
| `MISSING_EVIDENCE` | `EVIDENCE_ACQUISITION` | `EVIDENCE_AVAILABLE` | `EVIDENCE_VALIDATED` |
| `MISSING_EVIDENCE` with stale/unknown freshness | `EVIDENCE_ACQUISITION` | `FRESH_EVIDENCE_AVAILABLE` | `EVIDENCE_VALIDATED` |
| `UNKNOWN_STATE` | `OBSERVATION` | `STATE_OBSERVED` | `OBSERVATION_NORMALIZED` |
| `CONFLICT` | `CONFLICT_RESOLUTION` | `CONFLICT_RESOLVED` | `EXPLICIT_RESOLUTION_RECORDED` |
| `UNRESOLVED_INPUT` | `INPUT_ACQUISITION` | `EXPLICIT_INPUT_PROVIDED` | `INPUT_RECORDED` |
| `UNSUPPORTED_COMPARISON` | `NORMALIZATION` | `COMPARABLE_STATE_AVAILABLE` | `COMPARISON_SUPPORTED` |

`SATISFIED`, `UNSATISFIED_CONDITION`, `VIOLATION` and `MISSING_STATE` do not
produce a `PlanningInput`; their later handling remains a domain-change or
no-op concern. This preserves the invariant that uncertainty is not silently
turned into a repair action.

## Explicit constraints and traceability

Every input retains its `PlanningInputId` (equal to the source Delta-item
identity), DesiredState identity, Delta identity, Delta-item identity and
originating `ConditionId`. Its `RequiredOutcome` is copied from the Delta and
is validated against the input kind.

`InformationRequirements` carries only typed constraints and references:

- `FreshnessRequirement::Fresh` is emitted for stale or unknown freshness;
- an optional minimum `SensitivityClass` can be supplied explicitly by
  `PlanningInputRules`;
- evidence and provenance identities are retained in canonical order.

Raw evidence, secret material and model output are never copied into the
input rationale. A future evidence adapter must satisfy the stated metadata
and provenance contract; the presence of a planning input does not make a
source authoritative.

## Determinism and boundaries

`derive_planning_inputs` validates the Delta against the DesiredState, emits
one input per information Delta item, deduplicates and sorts references, and
returns inputs in typed identity order. `derive_planning_input` provides the
same rules for one source item. Unsupported IR versions, missing Delta items,
malformed references and mismatched required-outcome kinds fail closed.

Conflict resolution is an explicit outcome, not source precedence. Unknown
state requests observation, missing evidence requests evidence acquisition,
and unsupported comparison requests a supported normalization/comparison
path. No function in this module reads ambient time or invokes a provider.
