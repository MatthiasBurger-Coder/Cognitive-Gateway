# Three Amigos adaptation review

Date: 2026-10-10. Scope: EPIC-04.16 #297. Reviews below were performed as
sequential role passes by one agent. They are scenario reviews against inspected
repository evidence, not independent agents or automated skill-execution tests.

The adapted repository skill preserves the original four perspectives while
using CG governance, Rust ownership and actual quality commands. The adjacent
Tiny-Swarm-World skill is unchanged. The source and original SHA-256 are retained
in the skill's `references/source.md`.

| Scenario and observed evidence | Expected decision | Review result |
| --- | --- | --- |
| Baseline facade exposes resolve/context; product host denies them; injected tests pass | PARTIAL, product wiring required | Detected in GAP-01..03; #292 created before production edits |
| Session fixture returns pending/completed projections; #272/#273/#275 are planned | REQUIRES_REFINEMENT for session consumer; include foundation effort | Detected in GAP-04..09; #293 and #294 separated, no coordinator added to adapter |
| Synthetic fixture calls itself `codex / 1.0` | EXECUTABLE proof only, installed-client check required | Detected in GAP-10; real CLI 0.162.1 exposed protocol and discovery metadata incompatibilities |
| Actual client calls shipped host with admitted fixtures | INSTALLED_CLIENT evidence for exercised operations, substitutions declared | Separate runner now checks full CLI parity for inspection/resolve/explain/compile; does not claim session completion |
| Closed issue/PR says `Closes`; its report says NOT_COMPLETE and lifecycle absent | Broader scope remains open | #126 and new full qualification/acceptance issues remain open |
| Parent receives architecture criteria after original child graph | Re-extract parent, do not infer completion from closed children | Intake documents all nineteen original criteria and five later additions |
| Canonical service exists, trusted mapping understood, executable tests possible | Bounded slice ready without waiting for unrelated sessions | #292 implemented independently; existing canonical CLI regressions retained |
| User selects structured tasks but exact result/evidence source is unspecified | Preserve supported scope, clarify product success before implementing lifecycle | Accepted structured-only scope recorded; artifact vs desired-state question raised, no fabricated verified receipt |
| Target uses Rust, no TSW workflow engine, DEVELOPMENT mode | Use target toolchain/governance; no invented workflow dependency | Planning uses Rust services, repository gates and ordered issues |
| Root has no authorization to send messages or close incomplete work | Do not invent permission from skill | Issues created under explicit user instruction; no unsolicited issue comments or full-scope closure |

Structural validation was executed with the skill-creator validator and returned
`Skill is valid!`. The generated `agents/openai.yaml` supports discovery with the
same purpose and no explicit-only restriction. References resolve relative to
the skill directory. This review demonstrates concrete decisions on the EPIC-04
case; it does not establish that every future model invocation will obey the
skill or that the original skill caused earlier planning decisions.

See [delivery plan](epic-04-gap-plan.md) for historical facts, hypotheses,
prerequisite effort and issue ordering. Skill instructions prevent recurrence by
requiring composition-root tracing, actual prerequisite state, precise baseline
success, evidence labels and report/closure reconciliation before DONE claims.
