# CG-08.11: application API and downstream ownership

`DeclarativeResolutionApplication` exposes provider-neutral Rust operations:

- `resolve_plan(port, rules)` captures once and returns snapshot + report.
- `inspect_resolution` revalidates and returns the Plan, exact basis/rule identity,
  complete report and per-alternative policy inputs.
- `explain_resolution`, `validate_resolution`, `serialize_resolution` and
  `parse_resolution` delegate to the strict CG-08 contracts.
- `basis_is_current` validates the proposal and compares a newly captured basis.
- `inspect_v1_projection` checks an **externally proposed** CG-10 execution context;
  it never constructs one or evaluates policy.

Public result structs remain proposals when passed back into an API. Inspection,
projection and artifact operations revalidate them rather than trusting public
fields. Errors preserve snapshot, composition, artifact and trace categories.
No operation mutates process, catalog or project state, compiles a context, invokes
retrieval, or starts a runtime. CLI integration belongs to CG-11; no new CLI command
is introduced here.

## CG-09 input contract

Each `PolicyBindingInput` carries exactly one alternative, its complete required
capability contracts/classes (root and nested), and canonical process constraints.
The containing inspection retains the full Plan, original source restrictions,
Skill rules/closure, readiness and all basis/revision references. Exclusive
alternatives are **not unioned** into one policy request. Candidate/provided/required
capabilities are never labeled approved. The report's overall/step outcomes and
alternatives must remain visible to the consuming policy workflow.

CG-09 supplies `ExternalPolicyResult` using the existing `gateway-policy`
`PolicyDecision` enum. CG-08 only reads that result; it neither implements nor calls
a PolicyEvaluator. The caller must authenticate the result and its decision
reference. Its full basis and step must match. DENY, REQUIRE_CONSENT, missing policy,
missing required approvals or an unrelated approval list cannot become compatible
by virtue of resolver readiness.

## ExecutionContextIR v1 field mapping

| Existing v1 field | Owner and explicit required source |
| --- | --- |
| `schema_version` | CG-02: existing v1 constructors validate 1.0. |
| `id` | CG-10: externally assigned execution-context identity. |
| `task` | CG-10/caller: explicit task normalization; mapping binds TaskId and PlanStepId, never inferred from prose. |
| `workflow_id` | CG-02/CG-10: existing validated workflow plus owner-supplied Process-definition mapping decision. |
| `primary_agent_id` | CG-08: exact selected canonical primary Agent. |
| `skill_ids` | CG-08: complete dependency-first effective Skill closure; nonempty and exact workflow-compatible membership. |
| `operating_mode` | Captured scoped input, preserved independently. |
| `execution_profile` | Captured scoped input, preserved independently; FULL_PATH grants nothing. |
| `state` | CG-04/CG-02 through CG-10: externally proposed state triple validated by existing CG-02 constructors. CG-08 does not translate state names or approve a transition. |
| `policy_id` | CG-09 result; must match the context and existing workflow policy. |
| `knowledge_queries` | CG-03/CG-10: explicit canonical queries/projection input; no retrieval occurs here. |
| `approved_capability_ids` | Authenticated external CG-09 result only; exact context/result equality and required-capability coverage. |
| `constraints` | CG-10: supported CG-02 typed constraints plus retained original policy input. Unsupported intrinsic semantics cannot be dropped. |
| `target_runtime` | Caller/CG-10: opaque runtime identity, never selected or invoked by CG-08. |

The complete Plan, responsibility map, source constraints, process basis/revision,
rule identity and policy/mapping decision references remain in the handoff beside
the v1 context; v1 has no fields for those references. CG-10 and the execution
boundary must preserve that association and revalidate current CG-04/policy state.
`CompatibleV1Shape` means the supplied context passed structural and canonical
contract checks, **not** that a transition or execution is authorized.

## Compatibility matrix

| Resolution/proposal | Result |
| --- | --- |
| Process-bound, one Agent, nonempty workflow-compatible closure, matching owner mapping, external ALLOW and actual CG-02 validation | `CompatibleV1Shape`; still no execution grant. |
| No-op | `NoWork`; no context is manufactured, including with an active process. |
| Missing/ambiguous/partial/search-incomplete result | `Unresolved`. |
| No selected template | `NoTemplate`. |
| Empty effective closure | `EmptySkills`. |
| Multiple participating Agents or non-primary Skill responsibility | `MultipleAgents`. |
| Blocked/deferred/unknown readiness | `NotCurrentlyEligible`. |
| Missing/stale owner mapping, wrong task/workflow/role/mode/profile/closure/catalog | Explicit mapping/context/catalog problem. |
| Missing/denied/consent-required/stale/insufficient policy | Explicit policy problem; never implicit ALLOW. |
| Intrinsic restriction without a supported v1 representation | `UnmappedConstraints`. |

Read-only is representable only with approvals restricted to required INSPECT
contracts. Exact Mode/Profile semantics are preserved by those separate v1 fields.
Other intrinsic restrictions require an owner-sanctioned extension/mapping, not
an invented equivalent CG-02 constraint.

## Owner decision CG08-PROJECTION-01 — OPEN / blocking parent closure

Owners: **CG-02 (IR contract) and CG-10 (context projection)**.

Current v1 cannot losslessly represent arbitrary valid CG-08 no-template,
empty-Skill, multi-Agent or unmapped-constraint resolutions. The implementation
preserves them and emits typed incompatibilities. It does not invent a workflow,
activate unrelated Skills, flatten responsibilities or weaken existing v1 checks.

Required owner decision: sanction and prove a lossless mapping for each required
case, or approve an IR extension with migration and CG-10 mapping contracts.
No such extension is authorized or implemented in this PR. The synthetic mapping
decision in tests is a **contract fixture**, not a production approval. This open
decision blocks parent #7's projection criterion and the final all-PASS criteria
of #158. Completing CG-08.11 is not a claim of complete parent acceptance.

## Evidence

Six integration tests use real `ExecutionContextIR::new_v1` and `validate_against`
with CG-03 documents converted through their existing `to_domain` methods and an
explicit synthetic workflow/policy. Tests cover all API operations, immutable
revalidation, nested required contracts, positive projection, denied/missing policy,
no-template/empty-Skill/multi-Agent/role-or-closure mismatch, stale mapping/basis,
paused and no-op work and unsupported constraints. No synthetic catalog entries
are added to production. API integration also fixes the active-process/no-op
activity-demand regression with a focused test.

```powershell
cargo test -p gateway-application --test resolution_application
$env:CARGO_TARGET_DIR='D:\Projects\Cognitive-Gateway\target\cg08'
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
```

New API module: **338/343 lines (98.54%)**. Materially changed composition module:
**650/658 (98.78%)**. Workspace tests, fmt, Clippy and architecture guard pass.
