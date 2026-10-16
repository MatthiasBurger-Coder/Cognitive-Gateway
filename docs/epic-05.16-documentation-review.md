# EPIC-05.16 documentation review and evidence

Scope: #194, documentation needed to implement, operate at the delivered component
boundary and independently reproduce CGSL. Review basis:
`9b06a989ffa2a89de1b90b64385124b4967b593c`, initially clean checkout, DEVELOPMENT
mode, 2026-10-17. This candidate changes documentation only. It adds no runtime,
storage, dependencies, canonical schema or authority rules.

## Requirement and readiness review

The Three Amigos requirement gatekeeper was applied as requirement, architecture,
automation and tester role passes by one agent. No independent reviewer or
approval is claimed. Inputs were #194, parent #177 including its October 4/6
additions, delivered #193, repository governance, ADR-015, arc42, the language,
task and handoff specifications, source modules and executable fixtures.

Requirement pass: all deliverables already have contract/component foundations;
missing work is a coherent implementer entry point, migration guide, connected
reference walkthroughs and reconciliation of stale status statements. Architecture
pass: existing source, IR and Process/Policy ownership is sufficient; documentation
must distinguish shape validation, trusted admission and runtime proof. Automation
pass: use the existing offline Cargo suites, contract tests and architecture
guard; no new CLI or production implementation is needed. Tester pass: compare
the text with exact fixture operands and negative paths; run portable conformance
and downstream boundary tests and resolve local links.

Readiness decision: **READY_FOR_WORKFLOW** for this documentation scope.
Dependencies #179 and #193 are present with inspectable contracts and fixtures;
their issue state alone is not the evidence. Implementation order: index existing
normative contracts; document migration and reference continuations; reconcile
arc42/status; execute regressions and audit acceptance. Bounded effort estimate:
0.5–1 engineer-day documentation and 0.25–0.5 qualification, moderate confidence,
assuming existing #179–#193 components. Downstream producer/host implementation is
excluded. Rollback reverts these additive guides and documentation corrections.
Stop if a claimed example requires inventing runtime behavior or contract types.

## Acceptance matrix

All statuses below qualify #194's documentation deliverables, not full EPIC-05
runtime acceptance. Requirement and architecture review are source inspection;
component verification is executed test evidence.

| ID | Requirement/source | Production path / owner and dependency | Evidence | Status |
| --- | --- | --- | --- | --- |
| D1 | #194 new implementer needs no issue history; language and task contract published | CGSL frontend/domain; #179 vocabulary and #193 corpus present | [Implementer guide](cgsl-implementer-guide.md) indexes normative language, task schema, context, gaps, evidence, output, compiler stages and admission contracts | VERIFIED |
| D2 | #194 every core construct has a valid example | Existing frontend example admission and task domain | [Reference walkthroughs](cgsl-reference-examples.md) map all 20 constructs to complete source operands; sidecars are explicitly valid inspection syntax, not executable tasks; frontend/conformance suites pass | VERIFIED |
| D3 | #194 invalid and ambiguous cases documented | Frontend diagnostics, resolver, completeness and host ports | Example rejection manifest, portable captured contexts, negative-case table; interpretation/completeness and domain suites pass | VERIFIED |
| D4 | #194 Process IR / Prompt IR boundaries explicit; arc42 updated | CG-04 engine, CG-07/08 adapter, CG-10, future #263 | Exact process pin and gate test; both offline prompt renderings pass; arc42 strategy, blocks, runtime, concepts, decisions, risks and glossary reconciled | VERIFIED |
| D5 | #194 versioning, compatibility and migration guide | Independent source/task/reference readers | [Migration guide](cgsl-migration.md), normative version rules, #193 compatibility matrix; frozen unsupported-version and canonical roundtrip tests pass | VERIFIED |
| D6 | #194 docs match implementation and conformance; current vs future clear | Shipped Rust components, #193 fixtures; #285/#286/#263/#275/#279 remain downstream | Source/fixture review, status reconciliation, portable suite, planning/process/prompt regression and architecture/contract checks below | VERIFIED |

## Executed verification

Executed against the above base plus this documentation diff on 2026-10-17:

| Command | Result |
| --- | --- |
| `cargo test -p gateway-cgsl --all-targets --locked` | PASS: 20 tests; frozen corpus, generated properties and published examples |
| `cargo test -p gateway-application --test interpretation --test completeness --test semantic_planning --locked` | PASS: 41 tests; resolution, clarification/resumption, optional proposals and CG-06/07/08/Process boundary |
| `cargo test -p gateway-domain --test semantic_task_v1 --test semantic_resolution --test semantic_output --locked` | PASS: 17 tests; typed contracts and invariants |
| `cargo test -p gateway-context --test prompt_handoff_reference --locked` | PASS: 3 tests; equivalent offline layouts, missing metadata/authority rejection and provider independence |
| `python3 -m unittest discover -s tests/architecture -p 'test_*.py'` | PASS: 39 passed, one Windows-only WinDLL test skipped on Linux |
| `python3 -m unittest discover -s tests/contracts -p 'test_*.py'` | PASS: 17 tests |
| `bash scripts/check-architecture.sh` | PASS: dependency graph and architecture guard |
| Changed/new Markdown relative-link target audit and `git diff --check` | PASS: local targets exist; no whitespace errors |

No production code changed, so new production coverage is not applicable. Existing
#193 coverage evidence retains its original source binding and is not presented
as a new coverage run. Full workspace release qualification was not required for
this documentation change. Real effects, installed host, live provider calls,
canonical #285/#286 producer integration and a second independent implementation
were not run or qualified. Those limits are explicit in the guides and do not
reduce #194's documentation acceptance scope.

Completion decision: **VERIFIED for EPIC-05.16 documentation**. The public issue
state is unchanged by this local evidence record.
