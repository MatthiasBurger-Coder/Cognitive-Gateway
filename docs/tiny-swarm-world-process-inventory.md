# Tiny Swarm World Process Semantics Inventory

## Status and provenance

This is the CG-04.14 migration inventory. TSW is inspected as source evidence
only; it is not a runtime dependency of `gateway-process`. The source snapshot
is reproducible at commit `27ce3960da98a9ba124fd3f9ff5e003b13e89c60` of
`MatthiasBurger-Coder/Tiny-Swarm-World` (`docs(issue-252): record incomplete
RC1 audit`, 2026-08-21). The machine-readable record is
[`tiny-swarm-world-process-inventory.json`](tiny-swarm-world-process-inventory.json).

The snapshot contains workflow prompts and governance documents under
`.agents/`, executable platform workflow semantics under
`src/tiny_swarm_world/application/services/platform/workflow/`, and workflow /
S3D evidence under `documentation/` and `.codex/evidence/`. No TSW source is
copied into the canonical process catalog. Project paths, issue numbers,
service names, host topology and current runtime state remain external inputs.

## Classification rules

Every discovered process or governance semantic unit is assigned exactly one
classification:

| Classification | Meaning in this inventory |
| --- | --- |
| `REUSABLE_GENERIC` | A project-independent lifecycle rule suitable for a canonical process definition. |
| `MERGE_OR_DEDUPLICATE` | Semantics already represented by another unit; migrate one normalized form. |
| `PROJECT_BOUND_INPUT` | A fact, value or configuration supplied by a consuming project at runtime. |
| `DEPRECATED` | Historical or explicitly retired behavior that must not be migrated. |
| `EXCLUDED` | Source material outside Process Engine authority, such as runtime/provider configuration. |
| `DEFERRED_ENGINE_FEATURE` | Valuable generic semantics that require a later execution-graph extension. |
| `UNSUPPORTED_GAP` | Semantics with no safe v1 representation and no approved approximation. |

## Inventory and decisions

| ID | TSW provenance | Semantic intent | Dependencies | Project-specific assumptions | Capability signals | Candidate target / required CG feature | Classification | Decision and rationale |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `TSW-PROC-001` | `.agents/prompts/workflow-create.md`; `.agents/prompts/workflow-execute.md` | Create and execute a bounded workflow with explicit intake, validation, implementation, regression and evidence stages. | requirement intake; quality gates; evidence | issue IDs and repository paths are contextual | `workflow.execute`; `evidence.record` | `implementation-lifecycle`; states, events, transitions, evidence | `REUSABLE_GENERIC` | The lifecycle shape is generic; TSW prompt wording and operator commands are not migrated. |
| `TSW-PROC-002` | `.agents/skills/three-amigos-requirement-gatekeeper/SKILL.md` | Requirement readiness gate checks scope, architecture fit, testability, ownership and stop conditions. | workflow intake; architecture review; test evidence | source issue and project architecture are external inputs | `requirement.validate`; `quality.gate` | `requirement-readiness`; gate, evidence, authorization | `REUSABLE_GENERIC` | The decision criteria are reusable when represented as typed gate inputs. |
| `TSW-PROC-003` | `documentation/workflow/requirement-matrix.md`; `.agents/skills/three-amigos-requirement-gatekeeper/templates/` | Trace explicit and implicit requirements to implementation and evidence. | requirement readiness; evidence | issue-specific requirement IDs and file paths are external | `requirement.trace`; `evidence.record` | `evidence-completion`; evidence and invariant contracts | `REUSABLE_GENERIC` | Traceability is a reusable completion discipline; the source matrix itself is project-bound. |
| `TSW-PROC-004` | `.agents/skills/quality-gate-orchestrator/workflow.md`; `.agents/skills/quality-gate-orchestrator/quality-gates.md` | Run ordered quality gates and fail closed on failed, blocked or missing results. | test execution; architecture checks; evidence | command names, CI providers and repositories are external | `quality.verify`; `evidence.record` | `verification-quality-gate`; gate, evidence, blocker | `REUSABLE_GENERIC` | Gate outcome semantics migrate; concrete tools and commands remain adapters. |
| `TSW-PROC-005` | `.agents/skills/workflow-executor/SKILL.md`; `documentation/process/workflow-execute.md` | Execute a slice, record checkpoint evidence, and stop on failed D8 quality decisions. | S3D ordering; quality gate; checkpoint | workflow IDs, branches and commit hashes are external | `workflow.execute`; `evidence.record` | `implementation-lifecycle`; transition, invariant, evidence | `MERGE_OR_DEDUPLICATE` | This is the execution/evidence view of `TSW-PROC-001`, not a second canonical lifecycle. |
| `TSW-PROC-006` | `src/tiny_swarm_world/application/services/platform/workflow/semantics.py`; `.../types.py` | Classify init, reconcile, expose, repair, reset, destroy and verify operations by mutability and confirmation. | platform resources; verification | Incus/LXC, Swarm services and confirmation phrases are project-bound | `platform.inspect`; `platform.mutate`; `platform.repair` | no v1 canonical process; typed external operation inputs | `PROJECT_BOUND_INPUT` | The taxonomy describes a concrete platform adapter. Only generic confirmation/failure concepts may be reused, not these operations or resource names. |
| `TSW-PROC-007` | `.agents/skills/s3d-execution-orchestrator/SKILL.md`; `.codex/evidence/s3d-execution-plan.md` | Validate slice dependency graphs, topological order, lock conflicts and serial/parallel decisions. | slice metadata; ownership; handoff | slice IDs, worktrees and branch names are external | `workflow.plan`; `lock.inspect` | `execution-graph-extension`; DAG, groups, locks | `DEFERRED_ENGINE_FEATURE` | The semantics are generic scheduling inputs, but Process IR v1 intentionally does not schedule execution graphs. |
| `TSW-PROC-008` | `.agents/skills/s3d-execution-orchestrator/SKILL.md`; `.codex/evidence/s3d-execution-plan.md` | Reject unknown dependencies and cycles before execution. | dependency graph | source workflow metadata is external | `dependency.validate` | `execution-graph-extension`; graph validation | `DEFERRED_ENGINE_FEATURE` | Safe graph validation is extension-ready but not silently emulated through free text or lifecycle states. |
| `TSW-PROC-009` | `.agents/skills/s3d-execution-orchestrator/SKILL.md`; `.agents/orchestrator/routing-rules.md` | Coordinate execution groups, isolated worktrees and ownership handoffs. | dependency graph; branch governance | Git worktrees, agents and branches are external | `workspace.isolate`; `handoff.record` | `execution-graph-extension`; groups, locks, handoffs | `EXCLUDED` | This is orchestration/runtime coordination, not lifecycle authority owned by CG-04. |
| `TSW-PROC-010` | `src/tiny_swarm_world/application/services/platform/workflow/runtime.py`; `.../verify.py` | Record verification evidence and distinguish verified, blocked, failed-to-apply and failed-to-verify outcomes. | platform steps; evidence repository | platform target IDs and command output are external | `platform.verify`; `evidence.record` | `verification-quality-gate`; evidence, blocker, failure event | `REUSABLE_GENERIC` | Outcome distinction is reusable; Python classes, adapters and output formats are excluded. |
| `TSW-PROC-011` | `src/tiny_swarm_world/application/services/platform/workflow/runtime.py`; `.../verify.py` | Retry transient verification failures within a bounded budget. | verification result; retry policy | retry delay and platform target are external | `quality.verify`; `recovery.retry` | `repair-recovery`; retry and recovery policy | `REUSABLE_GENERIC` | Bounded retry maps directly to CG-04.11 recovery semantics. |
| `TSW-PROC-012` | `.agents/skills/quality-gate-orchestrator/failure-handling.md`; `src/tiny_swarm_world/application/services/platform/workflow/results.py` | Route apply, verify, blocked and failed outcomes without treating non-success as success. | quality gate; evidence | adapter failure payloads are external | `failure.route`; `evidence.record` | `typed-failure-routing`; blocker, failure transition, evidence | `REUSABLE_GENERIC` | Typed failure routing is reusable and must remain explicit. |
| `TSW-PROC-013` | `.agents/skills/quality-gate-orchestrator/SKILL.md`; `.agents/skills/release-baseline-governance-expert/` | Harden through freeze, explicit stop conditions, regression and evidence audit. | quality gate; requirement matrix | hardening mode and release baseline are external governance inputs | `quality.audit`; `evidence.audit` | `hardening-lifecycle`; gate, invariant, blocker | `REUSABLE_GENERIC` | The governance pattern is reusable; the TSW release names and policy files are not. |
| `TSW-PROC-014` | `documentation/workflow/workflow.md`; `documentation/release/` equivalents in source snapshot | Qualify a release only after ordered functional, recovery, quality and evidence gates. | implementation lifecycle; quality; evidence | release name, host matrix, services and live results are external | `release.qualify`; `evidence.audit` | `release-qualification`; gate, evidence, authorization | `REUSABLE_GENERIC` | Release qualification is a generic process template with all environment facts externalized. |
| `TSW-PROC-015` | `.agents/skills/branch-ci-governance-expert/`; `.agents/skills/release-branch-governance/` | Enforce branch, checkpoint, merge and CI ordering constraints. | Git; CI; quality | branch names, PR IDs and commits are project state | `repository.inspect`; `quality.verify` | `implementation-lifecycle`; external event/evidence inputs | `PROJECT_BOUND_INPUT` | The rule signals can be consumed as inputs, but Git/CI state cannot become canonical process definitions. |
| `TSW-PROC-016` | `.agents/skills/three-amigos-requirement-gatekeeper/`; `.agents/skills/workflow-conflict-resolution/` | Require explicit human/role review and authorization before advancing a gate. | requirement readiness; policy input | reviewer identities and issue context are external | `human.review`; `authorization.request` | `requirement-readiness`; authorization and policy guards | `REUSABLE_GENERIC` | Typed waiting/allow/deny inputs are already supported by CG-04.12; no reviewer implementation migrates. |
| `TSW-PROC-017` | `.agents/skills/workflow-conflict-resolution/`; `.agents/skills/quality-gate-orchestrator/failure-handling.md` | Escalate unresolved conflicts and stop-the-line conditions. | blocker; failure routing; authorization | escalation channels and recipients are external | `incident.escalate`; `blocker.resolve` | `typed-failure-routing`; blocker and recovery | `REUSABLE_GENERIC` | STOP is a generic blocked/failure outcome, not an implicit exception or free-text instruction. |
| `TSW-PROC-018` | `src/tiny_swarm_world/application/services/platform/workflow/` and platform service callers | Stream/distribute platform work and coordinate parallel resource operations. | execution groups; locks; platform adapters | concrete hosts, services, streams and resource handles | `platform.execute` | `execution-graph-extension`; stream metadata and parallelism | `UNSUPPORTED_GAP` | No safe v1 representation exists; it remains an explicit gap until an approved execution-graph extension exists. |
| `TSW-PROC-019` | `.codex/evidence/s3d-execution-plan.md`; S3D orchestration source | Join parallel branches at barriers before a lifecycle transition. | execution groups; dependency DAG | branch/worktree identity and scheduling state are external | `workflow.join` | `execution-graph-extension`; join/barrier | `DEFERRED_ENGINE_FEATURE` | Barrier semantics are valuable but must not be approximated by a normal lifecycle event. |
| `TSW-PROC-020` | Historical `.agents/roles/` and `.codex/agents/` runtime configuration | Select concrete agents, subagents or Python workflow implementations. | orchestration; provider runtime | agent identities, prompts, Python modules and runtime handles | `agent.resolve`; `runtime.execute` | none; CG-08/adapter boundary | `EXCLUDED` | Concrete execution selection belongs outside CG-04; capability requirements may be represented, provider selection may not. |

## Migration map and gaps

The approved candidate set for CG-04.15 is:

| Candidate Process Definition | Source units | Normalized v1 semantics |
| --- | --- | --- |
| `implementation-lifecycle` | `001`, `005`, `013`, `015` | intake/readiness → implementation → verification → evidence → completion, with explicit blockers and recovery |
| `requirement-readiness` | `002`, `003`, `016` | typed readiness gate, evidence trace and human/policy authorization boundary |
| `verification-quality-gate` | `004`, `010`, `011`, `012` | pass/fail/waiting/blocked verification with bounded retry and typed failure routing |
| `repair-recovery` | `011`, `012`, `017` | retry, repair, blocker and escalation semantics |
| `release-qualification` | `014`, `013` | ordered release gates and evidence discipline with project facts as external inputs |

The following are explicit CG-04.16 migration gaps and are not claimed to be
implemented by the candidate definitions: dependency DAG execution, cycle and
topological scheduling, execution groups, locks and lock conflicts,
parallelization, joins/barriers, stream distribution metadata and concrete
agent/runtime orchestration. The gap dispositions are machine-readable in the
JSON inventory and in the
[`CG-04.16 gap matrix`](execution-graph-migration-gaps.json); they must remain
visible in later catalog documentation.

## Boundary conclusion

TSW contributes reusable lifecycle rules and evidence discipline only. Its
Python modules, prompt files, platform resource taxonomy, Git/CI metadata,
service names, issue IDs and runtime agents are migration provenance or
external inputs. No canonical `gateway-process` component imports TSW, and no
unsupported execution-graph behavior is represented by a natural-language
fallback.
