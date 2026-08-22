# Hardening Workflow

## Purpose

This skill defines the normative execution path for Cognitive Gateway while the repository operates in `HARDENING` mode.

The workflow exists to prevent feature drift, uncontrolled cleanup, architectural guessing, false-green quality results, and changes that cannot be proven from repository evidence.

## Preconditions

The executor MUST read `.agents/AGENTS.md` before starting this workflow.

The active repository mode MUST be `HARDENING`.

The executor MUST NOT begin implementation until the hardening admission check has passed.

## Normative Path

```text
Request
  -> Root Governance
  -> OPERATING MODE
  -> Execution Profile Router
  -> Hardening Goal Lock
  -> Hardening Gate
  -> Hardening Admission Check
  -> S3
  -> S3D
  -> Role Distribution
  -> Smallest Correct Change
  -> Regression
  -> Quality
  -> Real E2E
  -> Evidence Audit
  -> Gate Decision
  -> next Gate | STOP-THE-LINE
```

## 1. Request

Capture the requested outcome without silently converting it into implementation scope.

## 2. Root Governance

Load repository governance and architecture authority before interpreting the request.

At minimum inspect:

- `.agents/AGENTS.md`;
- applicable architecture documentation under `docs/arc42/`;
- applicable ADRs under `docs/adr/`;
- the current Git state relevant to the slice.

## 3. OPERATING MODE

Confirm the active operating mode.

In `HARDENING` mode:

- feature freeze is active;
- repair, proof, simplification, reproducibility, evidence, and architecture alignment are allowed;
- product-scope expansion is not allowed without explicit governance approval.

## 4. Execution Profile Router

Resolve the execution profile before work begins.

For the current hardening phase the default is:

```yaml
execution_profile: FULL_PATH
```

A later governance slice may introduce additional profiles. HM-01 does not define alternate execution semantics.

## 5. Hardening Goal Lock

Write one concise statement of what must be proven or repaired.

The goal remains fixed for the run.

## 6. Hardening Gate

Reject work that is primarily feature development, speculative redesign, unrelated cleanup, or convenience refactoring.

## 7. Hardening Admission Check

Classify the work and answer every admission question from `.agents/AGENTS.md`.

If the admission check fails: `STOP-THE-LINE`.

## 8. S3

Perform structured static understanding of the affected slice before changing it.

S3 MUST establish at least:

- responsibilities;
- dependencies;
- inputs and outputs;
- contracts and invariants;
- test coverage or evidence gaps;
- architecture relation.

HM-01 defines the position of S3 in the workflow. Detailed S3 mechanics may be supplied by a dedicated skill later.

## 9. S3D

Perform dependency-aware decomposition of the intended change.

S3D identifies the smallest coherent change boundary and prevents unrelated files or responsibilities from being pulled into the slice.

HM-01 defines the position of S3D in the workflow. Detailed S3D mechanics may be supplied by a dedicated skill later.

## 10. Role Distribution

Assign only the roles actually required by the admitted work class.

Do not activate agents merely because they exist.

Roles must reduce uncertainty or produce required evidence. Redundant role activation is rejected.

## 11. Smallest Correct Change

Implement only the minimum change necessary to satisfy the locked goal while preserving architecture and contracts.

## 12. Regression

Run the most relevant regression checks for all affected behavior and boundaries.

A newly introduced failure blocks progression.

## 13. Quality

Run applicable repository quality gates.

Quality gates MUST NOT be weakened, skipped, muted, or reconfigured merely to obtain a green result.

## 14. Real E2E

When the slice affects executable system behavior, validate the real user-visible or system-visible path rather than relying solely on mocks or isolated tests.

If a real E2E check is not applicable, the evidence MUST state why.

## 15. Evidence Audit

Verify that the claimed result is supported by inspectable evidence.

Evidence may include test output, reproducible commands, logs, snapshots, architecture traceability, or repository changes, depending on the work class.

The evidence requirement is refined in HM-05. Until then, evidence MUST at minimum be sufficient for an independent reviewer to verify the gate decision.

## 16. Gate Decision

A gate decision is one of:

- `PASS` - the active hardening goal is satisfied and proven;
- `NEXT_GATE` - the current gate passed and an already-defined subsequent gate remains;
- `STOP-THE-LINE` - an unresolved blocker, contradiction, regression, evidence gap, or scope violation prevents safe continuation.

The executor MUST NOT report completion merely because code was changed.

## Work-Class Routing

Use the work classes defined in `.agents/AGENTS.md`:

- `H1` Defect Repair
- `H2` Architecture Hardening
- `H3` Test and Evidence Hardening
- `H4` Documentation and Governance Hardening
- `H5` Dependency or Build Hardening
- `FEATURE` Feature Work

`FEATURE` is rejected under the active feature freeze unless explicitly approved through governance.

## Non-Goals of HM-01

This skill deliberately does not yet define:

- executable workflow-engine integration;
- Codex-specific runtime rules;
- active-workflow metadata schema;
- the final evidence schema;
- detailed architecture-hardening checks;
- RC1 migration behavior;
- automated governance verification.

Those belong to HM-02 through HM-08 and MUST NOT be pulled into HM-01.
