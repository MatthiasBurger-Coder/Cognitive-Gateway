# Strict Cognitive Gherkin v1

This document is the normative source-language contract for CG-04.02. It
defines a readable, Gherkin-shaped input format for the Rust process compiler.
It does not define a Cucumber test suite and it never authorizes execution of
Cucumber step definitions.

## Authority and versioning

Only the declarations and statements in the tables below have process
authority. `Feature`, `Rule`, `Scenario`, `Given`, `When`, `Then`, `And` and
`But` provide structure and source locations. A sentence that is not one of
the listed forms is documentation only when it occurs in a `Comment`; in an
executable step position it is a compilation error.

Every source file must declare all three versions:

```gherkin
@process(canonical-issue-lifecycle)
@process-version(1)
@cg-language(1)
Feature: Canonical issue lifecycle
```

`@process`, `@process-version` and `@cg-language` are the only authoritative
metadata tags in v1. Tags with other names are rejected on authoritative
elements. The process identifier and version are mapped to
`ProcessDefinitionId` and `ProcessDefinitionVersion`; the language tag is
checked before any semantic mapping. Unsupported versions fail closed.

## Required declarations

The `Feature` must contain one `Rule: Process` section. Declarations are
written as exact `Given` forms in that rule, before behavior scenarios. The
following forms are mandatory:

```gherkin
Given state ANALYZE is initial
Given state COMPLETE is terminal
Given state IMPLEMENT
Given event implementation.accepted
Given gate THREE_AMIGOS
Given evidence verification.report
Given activity implement-change requires capability repository.write
Given invariant review-before-implementation requires gate THREE_AMIGOS passed
Given retry verification max 2 repair REPAIR
```

There must be exactly one initial state, at least one state, and at least one
event. Terminal states are explicit and cannot have outgoing transitions.
Declaration names are typed and must satisfy the identifier rules of the
Canonical Process IR. A declaration cannot be inferred from ordering, prose,
scenario names or a data table.

The complete v1 declaration vocabulary is:

| Form | IR element |
| --- | --- |
| `state <id>` / `state <id> is initial` / `state <id> is terminal` | `StateDefinition` |
| `event <id>` | `EventTypeDefinition` |
| `gate <id>` | `GateDefinition` |
| `evidence <id>` | `EvidenceRequirement` |
| `activity <id>` | `ActivityDefinition` |
| `activity <id> requires capability <capability-id>` | activity capability reference |
| `activity <id> produces evidence <evidence-id>` | activity output evidence |
| `activity <id> constrained by <name>=<value>` | typed activity constraint |
| `blocker <id> reason <text> resolvable` | `BlockerDefinition` |
| `invariant <id> requires gate <gate-id> passed` | `InvariantDefinition` |
| `retry <event-id> max <positive-integer> [repair <state-id>]` | `RecoveryPolicy` |

Repeated declarations merge only when they are byte-for-byte equivalent;
conflicting or duplicate declarations are diagnostics, not implicit merges.

## Behavioral vocabulary

Behavior is expressed in scenarios using exact statements. A scenario is a
source explanation and does not itself become an execution runtime.

```gherkin
Scenario: accept implementation
  Given process state ANALYZE
  Given gate THREE_AMIGOS is passed
  Given evidence requirements are present
  When event implementation.accepted occurs
  Then transition to state IMPLEMENT
  Then authorize activity implement-change

Scenario: verify a change
  Given process state VERIFY
  Given capability repository.read is available
  When event verification.failed occurs
  Then transition to state REPAIR
  Then block process with verification-failed
```

The finite statement set is:

| Statement | Meaning |
| --- | --- |
| `Given process state <state-id>` | transition source state |
| `Given gate <gate-id> is <status>` | typed gate guard |
| `Given evidence <evidence-id> is present` | typed evidence guard |
| `Given blocker <blocker-id> is active` | blocker constraint |
| `Given authorization <id> is <status>` | abstract authorization input |
| `Given policy decision <id> is <status>` | abstract policy input |
| `Given capability <capability-id> is available` | capability requirement guard |
| `When event <event-id> occurs` | transition event |
| `Then transition to state <state-id>` | transition target |
| `Then require gate <gate-id>` | transition gate dependency |
| `Then require evidence <evidence-id>` | transition evidence dependency |
| `Then authorize activity <activity-id>` | authorized activity projection |
| `Then require capability <capability-id>` | capability requirement |
| `Then block process with <blocker-id>` | blocker result |
| `Then pause process` | waiting lifecycle result |
| `Then complete process` | terminal lifecycle result |
| `Then retry activity max <positive-integer>` | bounded retry result |
| `Then repair through state <state-id>` | declared repair target |

`And` and `But` repeat the preceding statement family and do not change its
meaning. A `Rule` or `Scenario` without an authoritative statement is not a
transition. `Feature` and `Scenario` names are never identifiers.

## Data tables and tags

Data tables are allowed only as typed operands of a recognized statement. The
frontend preserves their source locations, but v1 accepts only the columns
required by that statement. An unrecognized column, free-form expression or
additional row is an error. Tags are metadata only unless they are one of the
three version/process tags shown above.

## Deterministic mapping and validation

The compiler performs these steps in order:

1. parse Gherkin structure and source locations;
2. require and validate the three version tags;
3. collect explicit declarations in source order;
4. resolve typed references against declarations;
5. map recognized behavior to IR fields;
6. sort canonical IR collections by their typed identifiers;
7. run the static Process IR validator and calculate the definition digest.

The same source and dependency contracts therefore produce the same IR and
digest. A semantic error includes a stable code, source location, offending
text and expected vocabulary.

Compilation fails for unknown steps, unknown declarations, malformed typed
identifiers, missing references, duplicate/conflicting declarations,
unsupported versions, ambiguous transitions, executable content, arbitrary
expressions or incompatible statement combinations. There is no heuristic,
natural-language, LLM, RAG or Cucumber-step fallback.

Events, evidence, activities, capabilities, authorization and policy inputs
remain separate typed concepts. Retrieval text is not evidence; an Agent or
Tool cannot grant a capability or mutate a process instance. Concrete Agent or
Skill selection is outside this language and belongs to later resolution
boundaries.

## Fixtures

`crates/gateway-process/fixtures/strict-cognitive-gherkin/valid.feature` is a
minimal valid source example. The files ending in `.feature` under the same
directory are intentionally invalid examples and define required frontend
negative cases:

- `invalid-unknown-step.feature`: unknown semantic text;
- `invalid-missing-initial.feature`: no explicit initial state;
- `invalid-version.feature`: unsupported language version;
- `invalid-reference.feature`: event/state reference not declared;
- `invalid-executable.feature`: embedded script/callback attempt.

These fixtures are source-language contracts, not Cucumber tests.
