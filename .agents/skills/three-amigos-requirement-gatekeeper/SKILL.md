---
name: three-amigos-requirement-gatekeeper
description: Review requirements before implementation or epic acceptance using requirement, architecture, automation and testing perspectives; identify delivery gaps, prerequisites, effort and evidence before planning slices or claiming completion.
---

# Three Amigos requirement and delivery gate

Use for an explicitly requested Three Amigos review, cross-plane integration
planning, or an epic completion audit. This review supports the user's task;
it does not authorize extra product scope or external writes.

## Authority and scope

Read the target repository's agent governance, the user-named epic and its
latest acceptance additions, applicable ADRs, architecture state, affected
source/contracts, shipped launchers and verified build/quality commands.
Use repository equivalents rather than requiring another project's paths,
framework, roles or workflow engine. Distinguish normative authority from
implemented behavior. Closed issues and merged PRs are status, not proof.

The gate reviews and refines requirements; production implementation follows
only after the affected slice is ready. Use available workflow tooling when
the repository requires it; otherwise record an ordered implementation plan.
Do not make nonexistent workflow skills a prerequisite. User authorization
already given remains valid; a routine design choice does not require another
permission request.

## Review perspectives

Record findings from each perspective and whether reviews are independent or
performed as role passes by one agent. Never invent reviewers or approvals.
Use delegation only when the user or applicable instructions authorize it.

- Requirement engineer: user-visible outcome, scope/non-goals, each criterion,
  late additions, assumptions, confidence, completion boundary.
- System architect: actual service ownership, ports, composition root, contracts,
  lifecycle/state ownership, authority, prerequisites, rollback and drift.
- Automation developer: repository's actual language/toolchain (Rust, Python,
  etc.), launcher wiring, installation/configuration, scripts and integration.
- Tester/evidence reviewer: positive and denied paths, determinism, failure and
  recovery, applicable gates/coverage, what each test actually establishes.

For multiple slices also examine dependency cycles, shared-file handoffs,
unstable contracts and the implementation owner of every prerequisite.
Ask: does the shipped implementation still satisfy the named epic?

## Trace the delivered path

For integration work read [delivery-gaps.md](references/delivery-gaps.md).
Follow the installed client or public command through the shipped launcher,
admission, adapter, facade, application service and persistence/effect boundary.
A configurable port or successful injected-host test does not establish that
the delivered entrypoint provides the behavior.

Produce a matrix with stable IDs:

| ID | Requirement/source | Production path/owner | Dependencies and actual state | Implementation evidence | Verification and evidence level | Status |
| --- | --- | --- | --- | --- | --- | --- |

Capture explicit and implicit behaviors and all named commands, services,
ports/contracts and evidence paths. Use OPEN, BLOCKED, PARTIAL or VERIFIED;
do not treat planned tests as executed evidence. Reconcile child issue criteria
with the complete parent, including later architecture-review additions.

## Plan the actual work

For each slice identify its product outcome, owning plane, contract changes,
storage/deployment impact, implementation files, tests, applicable coverage,
rollback and stop conditions. Include prerequisite implementation effort;
never estimate a bridge as if an absent host already existed.

Give effort ranges in engineer-days with assumptions and confidence; separate
implementation, qualification and prerequisite effort from elapsed calendar
time. State whether foundations are included. Research unknowns before refining
an estimate. Follow user-requested issue naming and create issues before
implementation only when issue creation is authorized. Link dependencies in
both the plan and epic; do not duplicate another plane's coordinator or services.

## Decisions

Return one decision for the reviewed scope and per-slice readiness:

- READY_FOR_WORKFLOW: no blocking requirements/ownership/contract ambiguity,
  verified commands, testable criteria, acyclic prerequisites with executable
  ordering. Planned foundations can be included explicitly; consumers cannot
  start until those foundations and their contracts pass their gate.
- PROCEED_WITH_ACCEPTED_ASSUMPTIONS: only documented, nonblocking assumptions
  accepted by the user remain. Identify the acceptance source.
- REQUIRES_REFINEMENT: identify the exact unresolved contract, owner, product
  behavior, environment or evidence prerequisite and the work/decision that
  resolves it. Continue independent authorized refinement. Do not freeze the
  whole task because a downstream slice is blocked. Ask only for information
  or decisions that cannot reasonably be resolved from authorized scope.

Do not equate confidence percentages with evidence. An external check with
missing prerequisites is NOT_RUN/BLOCKED, never PASS.

## Completion review

Revisit the matrix after implementation. All required behaviors must be verified
at their required evidence level against the delivered candidate. Evaluate
requirements, architecture and evidence separately. A green component gate
cannot close an issue whose required integration remains unimplemented.

Before closing an issue or epic, compare its full criteria with report status,
limitations, source revision and required runtime evidence. A report saying
NOT_COMPLETE, unsupported capabilities or missing required services blocks
closure of that scope. Track follow-on work without silently reducing the
original acceptance criteria. External state changes require authorization.
