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

## #297 candidate review

Rechecked on 2026-10-10 against base revision
`c69c1d169048bdf6a86ec6cab1a3cd534bef05c2` plus this skill/reference/report
change. This is a bounded planning improvement, with no production runtime
change. The original scenarios above retain their historical baseline; shared
services are now implemented, rather than absent in the current candidate.

Four sequential role passes by one agent produced these findings:

- Requirements: #297's six criteria are traced below. Parent #126 still has
  nineteen original criteria and five architecture additions; neither this
  skill nor closed children qualify them.
- Architecture: current `cg-local`/`cg-mcp` launchers use admitted local
  application wiring; `local_sessions.rs` constructs the PostgreSQL store and
  delegates lifecycle to application services. Historical missing-service
  findings must not be presented as the current dependency state.
- Automation: `references/repository-checks.md` now resolves CG governance,
  composition roots and actual Rust/Python qualification commands. The installed
  skill symlink resolves to this repository copy, with implicit discovery enabled.
- Testing/evidence: retained installed-client evidence names an older revision
  and declares EPIC-04 NOT_COMPLETE. The skill now explicitly requires candidate
  source bindings and required evidence levels per criterion. Missing reports
  deny closure; structural validation alone does not prove review behavior.

### Acceptance trace

| ID | #297 criterion | Implementation / evidence | Status |
| --- | --- | --- | --- |
| TA-01 | Demonstrated causes distinct from hypotheses | `epic-04-gap-plan.md`, "Why the gaps arose"; original source hash rechecked; #291 PR and latest #126 reread | VERIFIED |
| TA-02 | Four perspectives and target governance/toolchain | Skill review perspectives; `references/repository-checks.md`; role passes above | VERIFIED |
| TA-03 | Delivered path, prerequisite ledger, evidence matrix, parent reconciliation and foundation effort | Skill delivery/plan sections; explicit ledger in `references/delivery-gaps.md`; historical GAP-01..14 and 21–38 engineer-day plan including foundations | VERIFIED |
| TA-04 | Scope-consistent closure and NOT_COMPLETE blocking | Skill completion rule; candidate/source-binding rule; missing-evidence acceptance command below | VERIFIED |
| TA-05 | Realistic counterexamples, ready slice and honest provenance | Historical scenarios above and current boundary cases below; sequential role passes, no independent-review claim | VERIFIED |
| TA-06 | Structure/references, discoverable install and unchanged origin | Validator and relative-link check below; existing local symlink; `references/source.md` and unchanged original SHA-256 | VERIFIED |

### Current boundary cases

| Case reviewed | Decision and reason |
| --- | --- |
| Offer historical injected-host or synthetic `codex / 1.0` results to close shipped integration | REQUIRES_REFINEMENT: COMPONENT/EXECUTABLE evidence cannot replace required installed-client or shared-service proof |
| Declare a currently delivered foundation absent because its historical intake row is BLOCKED | Inspect current source and gate evidence; the historical row does not block independently ready planning work or prove current runtime qualification |
| Offer retained green installed-client report for the latest full-parent candidate | REQUIRES_REFINEMENT: older revision/source binding and explicit NOT_COMPLETE block the broader completion claim |
| Approve #297's bounded skill/documentation slice while full-parent qualification is incomplete | READY_FOR_WORKFLOW: scope, ownership, files and applicable checks are known, with no production service prerequisite; downstream parent completion remains separate |
| Treat all 24 parent requirements as satisfied when mandatory reports are missing | Closure denied by the actual runner: NOT_COMPLETE, `closure_allowed: false`, 24 BLOCKED requirements |

The prerequisite ledger for this bounded slice has no production foundations:

| Dependency / owner | Actual state | Required gate | Ordering / effort |
| --- | --- | --- | --- |
| Existing source skill / local skill maintainer | Present, original hash unchanged | Origin/hash check | Before adapting; included in the existing 1–2 engineer-day estimate |
| Repository authority and package / repository maintainer | Present, DEVELOPMENT / FULL_PATH; local discovery symlink resolves | Structure, links, applicable architecture/evidence regressions | Before acceptance of #297; qualification included in estimate |
| Shared-service and installed-client production qualification / #294–#296 owners | Retained evidence has scoped/historical boundaries | Full-parent candidate qualification | Not a dependency of the skill change; still required for parent Done |

### Executed checks

- `python3 /home/micro/.codex/skills/.system/skill-creator/scripts/quick_validate.py .agents/skills/three-amigos-requirement-gatekeeper`:
  PASS (`Skill is valid!`).
- Relative Markdown references in the skill package, repository paths in the
  new navigation reference, discovery symlink and original-source hash: PASS.
- `python3 -m unittest discover -s tests/architecture -p 'test_*.py'`:
  PASS, 31 tests. These test architecture and qualification invariants, not model
  compliance with the skill. Printed FAIL results belong to expected negative
  test fixtures; the suite result is OK.
- `bash scripts/check-architecture.sh`: PASS.
- `python3 scripts/qualify-epic04.py --output target/epic04-297-missing-evidence`:
  expected exit 1; NOT_COMPLETE, closure denied, all 24 requirements BLOCKED.
  This exercised the missing-input refusal, not runtime qualification.
- `git diff --check`: PASS.

No full production qualification was rerun for this skill/documentation change.
The scenario cases remain manual role-pass reviews, not automated skill-execution
tests. #297's bounded candidate is complete locally; merge and external issue
state are separate. Parent #126 remains NOT_COMPLETE. Rollback reverts this
skill/reference/report change without affecting runtime state or schemas.
