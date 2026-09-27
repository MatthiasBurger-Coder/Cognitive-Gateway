# CG-10 Context Compiler and ExecutionContext projection

`ContextApplication::compile_step` compiles one authorized, resolved plan step
into a provider-independent semantic context and the existing CG-02
`ExecutionContextIR`. It performs no retrieval, execution, lifecycle mutation,
clock access or provider prompt rendering.

## Implementation slices

| Slice | Implementation and evidence |
| --- | --- |
| CG-10.01 Typed context | `gateway-context::CompiledContext`, `ContextFragment`, `FragmentKind`, `FragmentMetadata` reuse CG-02 and CG-06 value objects. |
| CG-10.02 Selection and minimization | Explicit selected references, exact scope/step checks, canonical ordering, duplicate removal and conflict rejection. |
| CG-10.03 Trust and quality | External constructors enforce class/trust combinations and preserve `QualityMetadata`; memory requires a revision and validation reference. |
| CG-10.04 Context sources | `knowledge` retains `RetrievedKnowledge` bytes/source; `evidence` checks the CG-06 provenance link and emits references; memory uses the explicit external boundary. |
| CG-10.05 Semantic TAG | Stable catalog references, dynamic data, original input and gateway-generated task/output/constraint/state sections. |
| CG-10.06 Projection | Application revalidates CG-08 resolution, reevaluates CG-09 policy and checks its constructed v1 context against the existing CG-08/CG-02 compatibility boundary. |
| CG-10.07 Quality proof | Compiler/application integration tests, workspace checks and CI coverage gates at 95% per production file. |

For token bounded selection before this compilation boundary, see
[CG-20B context budgeting](context-budgeting.md).

## Application API

Use `gateway_application::context_application`:

1. Obtain a `ResolvedPlan` using the CG-08 application boundary.
2. Supply current, independently authenticated `PolicyAuthority` and
   `PolicyContext`, as described in [CG-09](policy-engine.md).
3. Supply the canonical `DefinitionCatalog` and a `ContextProjection`.
4. Construct external `ContextFragment` candidates with their source metadata,
   consuming scope and active step. Select only needed `ReferenceId`s.
5. Call `ContextApplication.compile_step(CompileStepInput { ... })`.
6. Consume `CompiledStep::context().execution_context()` for the existing v1
   contract, `to_json()` for the complete semantic envelope, and `explain()` for
   a summary without raw input or external payloads.

`ContextProjection` requires explicit context/runtime identities, normalized
`TaskDescriptor`, knowledge queries, a `WorkflowProjectionMapping`, and a
CG-02 `ExecutionState` with its resolution basis and mapping decision reference.
Arbitrary CG-04 state names are not translated into CG-02 enum values. The
caller supplies the approved state mapping. The compiler checks the mapping's
basis and retains its decision reference and the original process instance,
revision, state and activity when present.

A basis pins the plan, scope, situation, registry, process catalog and process
state. This is a consistency check, not authentication. The caller must obtain
current coherent snapshots and authenticate mapping decisions and policy facts.
Recompile after policy, consent, evidence or process changes. Compiled output is
not a reusable execution grant.

The selected workflow's policy must exactly match an authoritative policy that
was evaluated. All applicable policies contribute to evaluation and remain in
the envelope. Only the active binding's required capabilities, including Skill
closure requirements, enter the v1 approval list. The compiler cannot add
permissions, choose a different binding or create a legal process transition.
Typed policy constraints are retained in v1; source requirement, capability,
process and desired-state restrictions also remain in the envelope.

## Semantic envelope

The machine-readable envelope uses schema version 1 for CG-10 assembly. Its
`execution_context` member keeps CG-02's own schema version and semantics.

| Section | Content and trust |
| --- | --- |
| `stable.authority` | IDs of the applicable canonical policies. |
| `stable.workflow`, `agent`, `skills` | Selected canonical catalog references. |
| `dynamic` | Explicitly selected knowledge, evidence, memory or caller-input fragments with provenance and quality. |
| `user_input` | Original CG-06 input, as exact inline bytes or an explicit reference, classified `CALLER_INPUT`. Absent input stays null. |
| `gateway.task` | Explicit normalized task, distinct from original input. |
| `gateway.output_contract` | The active step's canonical completion and verification conditions. Desired-condition references remain references to the pinned upstream contract. |
| `gateway.constraints` | CG-02 typed constraints plus original active-step requirement/capability restrictions and applicable process/desired-state restrictions. |
| `gateway.policy` | The freshly evaluated step report and findings. |
| `gateway.runtime_state` | CG-02 state plus the captured CG-04 instance/revision/state/activity reference. |
| `gateway.provenance`, `basis` | Mapping decision references and immutable source identities/fingerprints. |
| `execution_context` | Validated existing CG-02 v1 execution contract. |

Catalog references remain stable; policy decisions, task, state, input and
selected data remain dynamic. No full Situation, observation set, process
history, conversation or unrelated plan step is copied. The original input is
included only when explicitly present in the captured CG-06 Intent.

`FragmentKind` identifies all twelve semantic classes. External constructors
accept only knowledge, evidence, memory and user-input data. Gateway-owned
classes come from the validated projection/application assembly. JSON here is
a machine-readable semantic document, not a provider request format. Future
renderers must preserve these classifications when producing XML, messages or
local templates.

## Selection, quality and duplicate rules

Selection is explicit and deterministic; the compiler does not infer relevance
from prose or probabilistic scores. Each selected fragment carries a nonempty
selection rationale and source, plus optional source revision, evidence links,
validation reference and the existing CG-06 `QualityMetadata` (trust,
sensitivity, confidence, freshness, uncertainty and conflict).

- Unselected candidates are omitted. Missing selected IDs fail compilation.
- Selected fragments must match the exact scope and active step.
- An identical ID with different content or metadata is an error.
- Identical repeated fragments are emitted once. Equal content under different
  IDs is coalesced only when kind, representation and all source/quality/selection
  metadata agree; the lexically first ID is retained.
- Equal content from different sources remains distinct, preserving provenance.
- Output fragments sort by semantic kind and reference ID. Queries and typed
  constraints are sorted and deduplicated. Resolver Skill ordering is retained
  because it already has canonical closure semantics.

Trust combinations are fixed: knowledge is `RetrievedContent`, evidence is
`ObservedEvidence`, memory is `DerivedAssessment`, and user data is `CallerInput`.
No external constructor accepts `CanonicalReference` or an authority class.
Retrieved content containing instruction-like text stays data and cannot alter
projection fields. Source metadata is supplied by trusted adapters; the compiler
does not independently authenticate external records.

Memory must carry a source revision and validation reference. This retains the
owning lifecycle's decision; it does not implement learning or validate memory
truth. Freshness/confidence/conflict metadata is preserved exactly. Stale or
uncertain material may be selected as context and never satisfies a policy or
process gate merely by being included. CG-09 owns evidence authorization.

The evidence adapter emits only the Evidence ID plus its verified provenance
reference and source information. The complete CG-06 evidence/provenance record
remains upstream. The retrieval adapter always takes source/revision from the
actual `RetrievedKnowledge`, preventing metadata from relabeling that source.

## Failure and version boundaries

Compilation returns typed errors for invalid resolution, denied/waiting policy,
stale mapping, unknown step, missing executable binding, catalog/policy mismatch,
invalid v1 projection or invalid fragment selection. NoOp has no executable
binding and produces no runtime context. Unsupported projection shapes fail
closed; CG-10 does not invent a workflow, Skill, primary Agent or constraint
mapping to make v1 executable.

The existing `ContextCompiler::inspect_handoff` still preserves v1/v2 handoffs;
`adapt_executable_v2` only adapts a validated executable envelope. Non-executable
v2 records remain available for inspection. This implementation does not change
those CG-02 contracts or claim all resolver shapes are executable.

`CompiledContext::assemble` is the pure lower-level assembly function for an
already constructed IR. It validates local IR invariants and data selection.
Use the application entry point for resolution/catalog/policy checks. Neither
function itself executes a runtime.

## Verification

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
bash scripts/check-architecture.sh
cargo llvm-cov -p gateway-context -p gateway-application \
  --test compiled_context --test budgeted_selection --test context_application \
  --test context_compiler --test v2_handoff \
  --json --output-path target/cg10-coverage.json
python3 scripts/check-context-coverage.py --self-test
python3 scripts/check-context-coverage.py target/cg10-coverage.json
```

The coverage gate checks every CG-10 production file separately at 95% and
rejects missing or duplicate report entries and invalid counts. CI runs the
same gate. When sharing a checkout with Windows, use a separate Linux
`CARGO_TARGET_DIR` and `CARGO_LLVM_COV_TARGET_DIR` to avoid mixed object formats.

### Recorded implementation evidence

The implementation was checked on Linux with workspace tests, Clippy with
warnings denied, formatting and the architecture guard. The focused suites
cover deterministic assembly, duplicate/conflict behavior, explicit selection,
source/trust preservation, original input, policy denial, stale mappings, NoOp,
unsupported template shapes and CG-02 JSON compatibility.

Measured line coverage with the command above:

| Production file | Covered / total | Coverage |
| --- | --- | --- |
| `gateway-context/src/lib.rs` | 24 / 25 | 96.00% |
| `gateway-context/src/compiled.rs` | 175 / 177 | 98.87% |
| `gateway-application/src/context_application.rs` | 224 / 224 | 100.00% |

Tests: [semantic context](../crates/gateway-context/tests/compiled_context.rs)
and [application integration](../crates/gateway-application/tests/context_application.rs).
